-- Authorization: IAM-style scoped role assignments.
--
-- The shape of `role_assignments` is the single most consequential decision in
-- this schema. A flat `user_roles(user_id, role_id)` table cannot express
-- "admin on this one project, viewer everywhere else", and retrofitting scope
-- onto it later means rewriting every authorization call site. So an assignment
-- is a triple from the start: (principal, role, scope).

CREATE TABLE permissions (
    key         TEXT PRIMARY KEY,
    description TEXT NOT NULL
) STRICT;

CREATE TABLE roles (
    id          TEXT    PRIMARY KEY,
    -- NULL means a built-in role shared by every organization. Non-NULL is a
    -- custom role belonging to one org.
    org_id      TEXT    REFERENCES organizations (id) ON DELETE CASCADE,
    key         TEXT    NOT NULL,
    name        TEXT    NOT NULL,
    description TEXT    NOT NULL DEFAULT '',
    is_system   INTEGER NOT NULL DEFAULT 0 CHECK (is_system IN (0, 1)),
    created_at  INTEGER NOT NULL,

    -- System roles are the ones with no owning org; keeping the two facts
    -- consistent stops a custom role from claiming built-in status.
    CHECK ((is_system = 1 AND org_id IS NULL) OR (is_system = 0 AND org_id IS NOT NULL))
) STRICT;

CREATE UNIQUE INDEX roles_system_key ON roles (key) WHERE org_id IS NULL;
CREATE UNIQUE INDEX roles_org_key    ON roles (org_id, key) WHERE org_id IS NOT NULL;

CREATE TABLE role_permissions (
    role_id        TEXT NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    permission_key TEXT NOT NULL REFERENCES permissions (key) ON DELETE RESTRICT,

    PRIMARY KEY (role_id, permission_key)
) STRICT;

CREATE TABLE service_accounts (
    id          TEXT    PRIMARY KEY,
    org_id      TEXT    NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    name        TEXT    NOT NULL,
    description TEXT    NOT NULL DEFAULT '',
    created_by  TEXT    REFERENCES users (id) ON DELETE SET NULL,
    created_at  INTEGER NOT NULL,
    disabled_at INTEGER,

    UNIQUE (org_id, name)
) STRICT;

CREATE TABLE api_tokens (
    id             TEXT    PRIMARY KEY,
    org_id         TEXT    NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    principal_kind TEXT    NOT NULL CHECK (principal_kind IN ('user', 'service_account')),
    principal_id   TEXT    NOT NULL,
    name           TEXT    NOT NULL,

    -- Why a separate, non-secret `prefix`: the secret is stored as an Argon2
    -- hash, which is salted, so it cannot be indexed or looked up by value.
    -- Presented tokens look like `jetr_<prefix>_<secret>`; we find the row by
    -- prefix, then verify the secret against the hash. Without this the server
    -- would have to Argon2-hash the candidate against every row in the table.
    prefix         TEXT    NOT NULL UNIQUE,
    secret_hash    TEXT    NOT NULL,

    expires_at     INTEGER,
    last_used_at   INTEGER,
    revoked_at     INTEGER,
    created_at     INTEGER NOT NULL
) STRICT;

CREATE INDEX api_tokens_by_principal
    ON api_tokens (org_id, principal_kind, principal_id);

CREATE TABLE role_assignments (
    id             TEXT    PRIMARY KEY,
    org_id         TEXT    NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,

    principal_kind TEXT    NOT NULL
                           CHECK (principal_kind IN ('user', 'team', 'service_account')),
    principal_id   TEXT    NOT NULL,

    role_id        TEXT    NOT NULL REFERENCES roles (id) ON DELETE CASCADE,

    -- The scope this grant applies to. Permissions flow *down* the hierarchy
    -- org -> project -> pipeline, so an org-scoped grant covers every project
    -- in it. There is no polymorphic FK here because scope_id points into one
    -- of three tables; integrity is enforced in jet-store, and org_id bounds
    -- the blast radius of any inconsistency.
    scope_kind     TEXT    NOT NULL CHECK (scope_kind IN ('org', 'project', 'pipeline')),
    scope_id       TEXT    NOT NULL,

    granted_by     TEXT    REFERENCES users (id) ON DELETE SET NULL,
    created_at     INTEGER NOT NULL,

    -- Granting the same role twice at the same scope is a no-op, not a second
    -- grant -- otherwise revocation would have to delete an unknown number of
    -- duplicate rows to actually take effect.
    UNIQUE (principal_kind, principal_id, role_id, scope_kind, scope_id)
) STRICT;

-- The hot path: "every assignment for this principal in this org".
CREATE INDEX role_assignments_by_principal
    ON role_assignments (org_id, principal_kind, principal_id);
-- The reverse question, for "who has access to this project?" screens and for
-- cascading cleanup when a scope is deleted.
CREATE INDEX role_assignments_by_scope
    ON role_assignments (scope_kind, scope_id);

CREATE TABLE invitations (
    id          TEXT    PRIMARY KEY,
    org_id      TEXT    NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    email       TEXT    NOT NULL,

    -- An invitation carries the role *and* the scope, so a contractor can be
    -- invited straight into one project rather than being given org-wide access
    -- and narrowed afterwards.
    role_id     TEXT    NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    scope_kind  TEXT    NOT NULL CHECK (scope_kind IN ('org', 'project', 'pipeline')),
    scope_id    TEXT    NOT NULL,

    -- Hash only. A readable invitation token in the database means anyone with
    -- read access, a backup, or a leaked log can join the org.
    token_hash  TEXT    NOT NULL,

    invited_by  TEXT    REFERENCES users (id) ON DELETE SET NULL,
    -- Not nullable: an invitation that never expires is a permanent credential.
    expires_at  INTEGER NOT NULL,
    accepted_at INTEGER,
    accepted_by TEXT    REFERENCES users (id) ON DELETE SET NULL,
    revoked_at  INTEGER,
    created_at  INTEGER NOT NULL
) STRICT;

-- At most one live invitation per address per org, so re-inviting replaces
-- rather than accumulating usable tokens.
CREATE UNIQUE INDEX invitations_pending_key
    ON invitations (org_id, lower(email))
    WHERE accepted_at IS NULL AND revoked_at IS NULL;

CREATE TABLE audit_log (
    id          TEXT    PRIMARY KEY,
    -- Deliberately NOT a foreign key. Two reasons, both load-bearing:
    --
    --  * an audit record is an immutable statement that something happened, and
    --    it must outlive the organization it describes -- "who deleted this
    --    tenant, and when" is precisely the question an auditor asks;
    --  * a cascading delete would collide with the append-only triggers below
    --    and make deleting an organization fail outright.
    --
    -- So org_id is a plain tenant tag here, and deleting an org archives its
    -- audit trail rather than shredding it.
    org_id      TEXT    NOT NULL,
    actor_kind  TEXT    NOT NULL
                        CHECK (actor_kind IN ('user', 'service_account', 'system')),
    actor_id    TEXT,
    action      TEXT    NOT NULL,
    target_kind TEXT,
    target_id   TEXT,
    -- JSON. Deliberately schemaless: what is worth recording differs per action
    -- and must not require a migration to extend.
    metadata    TEXT,
    ip          TEXT,
    created_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX audit_log_by_org_time ON audit_log (org_id, created_at DESC);
CREATE INDEX audit_log_by_target   ON audit_log (target_kind, target_id);

-- Append-only, enforced by the database rather than by convention.
--
-- An audit trail that the application can edit is not an audit trail. These
-- triggers mean that even a bug -- or an attacker with the app's own database
-- credentials -- cannot quietly rewrite history through the normal interface.
-- Retention pruning, when it exists, will need a deliberate migration that
-- drops and recreates these, which is exactly the amount of friction it should
-- have.
CREATE TRIGGER audit_log_immutable_update
BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only: updates are not permitted');
END;

CREATE TRIGGER audit_log_immutable_delete
BEFORE DELETE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only: deletes are not permitted');
END;

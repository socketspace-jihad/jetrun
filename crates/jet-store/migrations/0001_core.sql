-- Identity and tenancy.
--
-- Conventions used throughout every migration:
--
--  * ids are 26-char Crockford base32 ULIDs stored as TEXT. Sortable by
--    creation time, so `ORDER BY id` is chronological and inserts land at the
--    right edge of the index instead of scattering like UUIDv4.
--  * timestamps are INTEGER Unix milliseconds, never TEXT and never a DB-native
--    date type. Unambiguous, timezone-free, sortable, and identical in meaning
--    under SQLite and Postgres -- which matters because the SaaS path swaps the
--    backend and must not reinterpret existing rows.
--  * every tenant-scoped row carries org_id even where it is reachable via a
--    join. Denormalized on purpose: authorization filters by org on nearly
--    every query, and it makes "delete this tenant" a bounded operation.
--  * `status`-style columns use CHECK constraints rather than a lookup table.
--    The set is small, closed, and known at compile time.

CREATE TABLE users (
    id          TEXT    PRIMARY KEY,
    email       TEXT    NOT NULL,
    name        TEXT    NOT NULL,
    status      TEXT    NOT NULL DEFAULT 'active'
                        CHECK (status IN ('active', 'suspended', 'deleted')),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

-- Users are global, not org-scoped: one human in two organizations is one row
-- with two memberships, not two accounts. Case-insensitive uniqueness because
-- nobody believes Alice@ and alice@ are different people, and the partial
-- predicate lets a deleted account's address be reclaimed.
CREATE UNIQUE INDEX users_email_key
    ON users (lower(email))
    WHERE status <> 'deleted';

CREATE TABLE credentials (
    id           TEXT    PRIMARY KEY,
    user_id      TEXT    NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind         TEXT    NOT NULL
                         CHECK (kind IN ('password', 'oidc', 'saml', 'ssh_key')),
    -- Federated identities: the IdP and its `sub` claim. NULL for passwords.
    provider     TEXT,
    subject      TEXT,
    -- Argon2 PHC string for passwords, public key for ssh_key, NULL for
    -- federated logins where the IdP holds the secret.
    secret_hash  TEXT,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER,

    -- A federated credential is meaningless without both halves of its identity.
    CHECK (
        (kind IN ('oidc', 'saml') AND provider IS NOT NULL AND subject IS NOT NULL)
        OR (kind IN ('password', 'ssh_key') AND secret_hash IS NOT NULL)
    )
) STRICT;

-- One external identity maps to exactly one user, or an attacker who can create
-- an account at the IdP could attach to someone else's.
CREATE UNIQUE INDEX credentials_federated_key
    ON credentials (kind, provider, subject)
    WHERE subject IS NOT NULL;

CREATE INDEX credentials_by_user ON credentials (user_id);

CREATE TABLE organizations (
    id         TEXT    PRIMARY KEY,
    slug       TEXT    NOT NULL UNIQUE,
    name       TEXT    NOT NULL,
    plan       TEXT    NOT NULL DEFAULT 'self-hosted',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE memberships (
    org_id    TEXT    NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id   TEXT    NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status    TEXT    NOT NULL DEFAULT 'active'
                      CHECK (status IN ('active', 'suspended')),
    joined_at INTEGER NOT NULL,

    PRIMARY KEY (org_id, user_id)
) STRICT;

-- Authorization asks "which orgs is this user in?" on every request.
CREATE INDEX memberships_by_user ON memberships (user_id);

CREATE TABLE teams (
    id         TEXT    PRIMARY KEY,
    org_id     TEXT    NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    slug       TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    created_at INTEGER NOT NULL,

    UNIQUE (org_id, slug)
) STRICT;

CREATE TABLE team_members (
    team_id  TEXT    NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id  TEXT    NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL,

    PRIMARY KEY (team_id, user_id)
) STRICT;

-- Resolving a principal's teams is on the hot authorization path.
CREATE INDEX team_members_by_user ON team_members (user_id);

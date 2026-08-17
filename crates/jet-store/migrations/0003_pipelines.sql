-- Projects, pipelines, runs, steps, secrets, workers.
--
-- `projects` and `pipelines` are also the non-org scope targets that
-- role_assignments points at, which is why they carry org_id directly: an
-- authorization check must resolve a scope's owning org without a join.

CREATE TABLE projects (
    id          TEXT    PRIMARY KEY,
    org_id      TEXT    NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    slug        TEXT    NOT NULL,
    name        TEXT    NOT NULL,
    description TEXT    NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    archived_at INTEGER,

    UNIQUE (org_id, slug)
) STRICT;

CREATE TABLE pipelines (
    id         TEXT    PRIMARY KEY,
    org_id     TEXT    NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    project_id TEXT    NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    slug       TEXT    NOT NULL,
    name       TEXT    NOT NULL,

    -- Digest of the YAML definition in the CAS. The definition itself is
    -- content-addressed rather than stored here, so a run records exactly which
    -- bytes it executed and an edited pipeline cannot retroactively change the
    -- history of past runs.
    definition_digest TEXT,

    created_at  INTEGER NOT NULL,
    archived_at INTEGER,

    UNIQUE (project_id, slug)
) STRICT;

CREATE TABLE runs (
    id          TEXT    PRIMARY KEY,
    org_id      TEXT    NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    pipeline_id TEXT    NOT NULL REFERENCES pipelines (id) ON DELETE CASCADE,

    -- Per-pipeline monotonic counter, the number humans actually refer to
    -- ("run #482"). Allocated inside the same transaction as the insert.
    number      INTEGER NOT NULL,

    status      TEXT    NOT NULL
                        CHECK (status IN ('queued', 'running', 'success',
                                          'failed', 'cancelled', 'timed_out')),

    trigger_kind TEXT   NOT NULL
                        CHECK (trigger_kind IN ('manual', 'webhook', 'schedule', 'api')),
    trigger_ref  TEXT,
    commit_sha   TEXT,

    -- Which definition this run actually executed; see pipelines above.
    definition_digest TEXT,

    created_by_kind TEXT CHECK (created_by_kind IN ('user', 'service_account', 'system')),
    created_by_id   TEXT,

    created_at  INTEGER NOT NULL,
    started_at  INTEGER,
    finished_at INTEGER,

    UNIQUE (pipeline_id, number)
) STRICT;

-- Run lists are always "latest first, for this pipeline".
CREATE INDEX runs_by_pipeline_time ON runs (pipeline_id, created_at DESC);
-- The scheduler's poll: what is waiting to be placed.
CREATE INDEX runs_pending ON runs (status, created_at) WHERE status IN ('queued', 'running');

CREATE TABLE steps (
    id     TEXT NOT NULL PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    run_id TEXT NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    name   TEXT NOT NULL,

    status TEXT NOT NULL
                CHECK (status IN ('pending', 'ready', 'running', 'success',
                                  'failed', 'skipped', 'cancelled', 'timed_out')),

    -- JSON array of step names this one waits on. Denormalized rather than an
    -- edge table: the DAG is small, always loaded whole, and never queried by
    -- edge.
    needs TEXT NOT NULL DEFAULT '[]',

    -- Cache accounting. Storing both keys is what makes "why did this step not
    -- hit the cache?" an answerable question instead of a support ticket --
    -- compare the action keys of two runs and the differing input is right
    -- there. This is a product feature, not just diagnostics.
    step_identity TEXT,
    action_key    TEXT,
    cache_outcome TEXT CHECK (cache_outcome IN ('miss', 'hit', 'manifest_miss', 'uncacheable')),

    -- Digest of the output tree, so a skipped step can still be replayed.
    output_tree TEXT,
    exit_code   INTEGER,

    worker_id   TEXT,
    created_at  INTEGER NOT NULL,
    started_at  INTEGER,
    finished_at INTEGER,

    UNIQUE (run_id, name)
) STRICT;

CREATE INDEX steps_by_run ON steps (run_id);
-- Cache hit-rate reporting, and finding prior runs of the same action.
CREATE INDEX steps_by_action_key ON steps (action_key) WHERE action_key IS NOT NULL;

CREATE TABLE secrets (
    id         TEXT    PRIMARY KEY,
    org_id     TEXT    NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    scope_kind TEXT    NOT NULL CHECK (scope_kind IN ('org', 'project', 'pipeline')),
    scope_id   TEXT    NOT NULL,
    name       TEXT    NOT NULL,

    -- Monotonic per-secret version. This is what goes into an action key --
    -- never the value. Hashing the value would let anyone with cache read
    -- access confirm guesses by probing for digests; omitting it entirely would
    -- mean rotating a secret never invalidates anything and stale artifacts
    -- linger. (secret_id, version) gives invalidation without disclosure.
    version    INTEGER NOT NULL,

    ciphertext BLOB    NOT NULL,
    nonce      BLOB    NOT NULL,

    created_by TEXT    REFERENCES users (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,

    UNIQUE (org_id, scope_kind, scope_id, name, version)
) STRICT;

CREATE TABLE workers (
    id       TEXT NOT NULL PRIMARY KEY,
    org_id   TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    name     TEXT NOT NULL,

    -- Machine shape (cpu/mem/arch), as JSON so new dimensions do not need a
    -- migration.
    shape    TEXT NOT NULL DEFAULT '{}',
    status   TEXT NOT NULL
                  CHECK (status IN ('registering', 'idle', 'busy', 'draining', 'gone')),

    -- Cache-affinity placement: the scheduler prefers a worker that already
    -- holds the relevant objects locally, so it needs a cheap way to ask what a
    -- worker has. JSON summary rather than a per-object table, which would be
    -- millions of rows for no benefit.
    cache_summary TEXT NOT NULL DEFAULT '{}',

    last_heartbeat_at INTEGER,
    registered_at     INTEGER NOT NULL,

    UNIQUE (org_id, name)
) STRICT;

-- Reaping dead workers scans by heartbeat.
CREATE INDEX workers_by_heartbeat ON workers (status, last_heartbeat_at);

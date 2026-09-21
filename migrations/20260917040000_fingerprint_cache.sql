-- Fingerprint cache for content-addressable build skipping
-- Primary key on hash gives O(1) lookup
CREATE TABLE IF NOT EXISTS fingerprint_cache (
    hash       TEXT PRIMARY KEY,
    project_id UUID NOT NULL,
    step_name  TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_fingerprint_project ON fingerprint_cache(project_id);

-- System settings: key-value store for runtime configuration.
-- Workers poll this table for config changes.
CREATE TABLE IF NOT EXISTS system_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed defaults
INSERT INTO system_settings (key, value) VALUES
    ('worker.max_parallel', '0'),           -- 0 = auto-detect (num_cpus)
    ('worker.tmpfs_enabled', 'true'),       -- build workspace in RAM
    ('worker.tmpfs_size_mb', '4096'),       -- 4GB tmpfs
    ('worker.dep_cache_enabled', 'true'),   -- persistent dependency cache
    ('worker.cpu_pinning_enabled', 'true'), -- cgroup CPU pinning
    ('worker.memory_limit_mb', '2048')      -- per-build memory limit
ON CONFLICT (key) DO NOTHING;

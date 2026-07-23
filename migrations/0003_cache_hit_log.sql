CREATE TABLE IF NOT EXISTS cache_hit_log (
    id              TEXT PRIMARY KEY,
    ts              INTEGER NOT NULL,
    model           TEXT NOT NULL,
    cache_hit       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_cache_hit_ts ON cache_hit_log(ts);

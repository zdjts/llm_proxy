CREATE TABLE IF NOT EXISTS audit_hourly (
    hour                INTEGER NOT NULL,
    model               TEXT    NOT NULL,
    pool_id             TEXT    NOT NULL,
    key_hash            TEXT    NOT NULL,
    cache_source        TEXT,
    error_code          TEXT,
    finish_reason       TEXT,

    request_count       INTEGER NOT NULL DEFAULT 0,
    success_count       INTEGER NOT NULL DEFAULT 0,
    retry_total         INTEGER NOT NULL DEFAULT 0,

    prompt_tokens       INTEGER NOT NULL DEFAULT 0,
    completion_tokens   INTEGER NOT NULL DEFAULT 0,
    total_tokens        INTEGER NOT NULL DEFAULT 0,
    cached_tokens       INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens    INTEGER NOT NULL DEFAULT 0,
    audio_tokens        INTEGER NOT NULL DEFAULT 0,

    latency_ms_sum      INTEGER NOT NULL DEFAULT 0,
    ttft_ms_sum         INTEGER NOT NULL DEFAULT 0,
    stream_count        INTEGER NOT NULL DEFAULT 0,

    PRIMARY KEY (hour, model, pool_id, key_hash, cache_source, error_code, finish_reason)
);
CREATE INDEX IF NOT EXISTS idx_audit_hourly_ts ON audit_hourly(hour);
CREATE INDEX IF NOT EXISTS idx_audit_hourly_model ON audit_hourly(model);
CREATE INDEX IF NOT EXISTS idx_audit_hourly_pool ON audit_hourly(pool_id);

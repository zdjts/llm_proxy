CREATE TABLE IF NOT EXISTS request_log (
    id              TEXT PRIMARY KEY,
    ts              INTEGER NOT NULL,
    client_ip       TEXT,
    model           TEXT NOT NULL,
    pool_id         TEXT NOT NULL,
    key_hash        TEXT NOT NULL,
    upstream        TEXT,
    status_code     INTEGER,
    latency_ms      INTEGER,
    prompt_tokens   INTEGER,
    completion_tokens INTEGER,
    total_tokens    INTEGER,
    is_stream       INTEGER NOT NULL,
    error           TEXT,
    cached_tokens          INTEGER DEFAULT NULL,
    cache_creation_tokens  INTEGER DEFAULT NULL,
    cache_source           TEXT    DEFAULT NULL,
    reasoning_tokens       INTEGER DEFAULT NULL,
    audio_tokens           INTEGER DEFAULT NULL,
    ttft_ms                INTEGER DEFAULT NULL,
    upstream_model         TEXT    DEFAULT NULL,
    system_fingerprint     TEXT    DEFAULT NULL,
    finish_reason          TEXT    DEFAULT NULL,
    error_code             TEXT    DEFAULT NULL,
    retry_count            INTEGER DEFAULT 0,
    tenant_id              TEXT    DEFAULT 'default'
);

CREATE INDEX IF NOT EXISTS idx_request_log_ts ON request_log(ts);
CREATE INDEX IF NOT EXISTS idx_request_log_model ON request_log(model);
CREATE INDEX IF NOT EXISTS idx_request_log_key_hash ON request_log(key_hash);

-- 0017: make audit_hourly dimensions NULL-safe and add cost/upstream model data.
--
-- SQLite PRIMARY KEY columns may still contain NULL unless they are explicitly
-- declared NOT NULL.  The original table therefore allowed duplicate logical
-- buckets whenever one of its dimensions was NULL. Rebuild it with canonical
-- empty-string dimension values and a real UNIQUE constraint.
ALTER TABLE audit_hourly ADD COLUMN upstream_model TEXT DEFAULT NULL;
ALTER TABLE audit_hourly ADD COLUMN cost_usd REAL DEFAULT NULL;

CREATE TABLE audit_hourly_new (
    hour                INTEGER NOT NULL,
    model               TEXT    NOT NULL,
    pool_id             TEXT    NOT NULL,
    key_hash            TEXT    NOT NULL,
    cache_source        TEXT    NOT NULL DEFAULT '',
    error_code          TEXT    NOT NULL DEFAULT '',
    finish_reason       TEXT    NOT NULL DEFAULT '',
    upstream_model      TEXT    NOT NULL DEFAULT '',
    tenant_id           TEXT    NOT NULL DEFAULT 'default',

    request_count       INTEGER NOT NULL DEFAULT 0,
    success_count       INTEGER NOT NULL DEFAULT 0,
    retry_total         INTEGER NOT NULL DEFAULT 0,

    prompt_tokens       INTEGER NOT NULL DEFAULT 0,
    completion_tokens   INTEGER NOT NULL DEFAULT 0,
    total_tokens        INTEGER NOT NULL DEFAULT 0,
    cached_tokens       INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens    INTEGER NOT NULL DEFAULT 0,
    audio_tokens        INTEGER NOT NULL DEFAULT 0,

    cost_usd            REAL    NOT NULL DEFAULT 0,
    latency_ms_sum      INTEGER NOT NULL DEFAULT 0,
    ttft_ms_sum         INTEGER NOT NULL DEFAULT 0,
    stream_count        INTEGER NOT NULL DEFAULT 0,

    UNIQUE (hour, model, pool_id, key_hash, cache_source, error_code,
            finish_reason, upstream_model, tenant_id)
);

INSERT INTO audit_hourly_new (
    hour, model, pool_id, key_hash, cache_source, error_code, finish_reason,
    upstream_model, tenant_id, request_count, success_count, retry_total,
    prompt_tokens, completion_tokens, total_tokens, cached_tokens,
    reasoning_tokens, audio_tokens, cost_usd, latency_ms_sum, ttft_ms_sum,
    stream_count
)
SELECT
    hour, model, pool_id, key_hash,
    COALESCE(cache_source, ''),
    COALESCE(error_code, ''),
    COALESCE(finish_reason, ''),
    COALESCE(upstream_model, ''),
    COALESCE(tenant_id, 'default'),
    SUM(request_count), SUM(success_count), SUM(retry_total),
    SUM(prompt_tokens), SUM(completion_tokens), SUM(total_tokens),
    SUM(cached_tokens), SUM(reasoning_tokens), SUM(audio_tokens),
    SUM(COALESCE(cost_usd, 0)), SUM(latency_ms_sum), SUM(ttft_ms_sum),
    SUM(stream_count)
FROM audit_hourly
GROUP BY hour, model, pool_id, key_hash, cache_source, error_code,
         finish_reason, upstream_model, tenant_id;

DROP TABLE audit_hourly;
ALTER TABLE audit_hourly_new RENAME TO audit_hourly;

CREATE INDEX idx_audit_hourly_ts ON audit_hourly(hour);
CREATE INDEX idx_audit_hourly_model ON audit_hourly(model);
CREATE INDEX idx_audit_hourly_pool ON audit_hourly(pool_id);
CREATE INDEX idx_audit_hourly_tenant ON audit_hourly(tenant_id);
CREATE INDEX idx_audit_hourly_upstream_model ON audit_hourly(upstream_model);

INSERT OR IGNORE INTO schema_version (version, description)
VALUES (17, '0017_audit_hourly_dimensions: NULL-safe dimensions and cost data');

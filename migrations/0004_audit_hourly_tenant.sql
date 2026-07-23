-- ADR-012 §2.2: add tenant_id dimension to audit_hourly.
ALTER TABLE audit_hourly ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';
CREATE INDEX IF NOT EXISTS idx_audit_hourly_tenant ON audit_hourly(tenant_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_audit_hourly_unique ON audit_hourly(
    hour, model, pool_id, key_hash, cache_source, error_code, finish_reason, tenant_id
);

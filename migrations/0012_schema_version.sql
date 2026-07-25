-- 0012: Schema version tracking (T140)
-- Part of v3.0 full-stack upgrade per docs/BRIEF-v3.0-fullstack-upgrade.md

CREATE TABLE IF NOT EXISTS schema_version (
    version     INTEGER PRIMARY KEY,
    applied_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    description TEXT
);

-- Record all migrations already applied (inferred from existing migration files)
INSERT OR IGNORE INTO schema_version (version, description) VALUES
    (1, '0001_init: request_log'),
    (2, '0002_audit_hourly'),
    (3, '0003_cache_hit_log'),
    (4, '0004_audit_hourly_tenant'),
    (5, '0005_alert_event'),
    (6, '0006_user_agent_cost'),
    (7, '0007_client_key_store'),
    (8, '0008_provider_config'),
    (9, '0009_rbac'),
    (10, '0010_prompt_audit_alert'),
    (11, '0011_fts5'),
    (12, '0012_schema_version');

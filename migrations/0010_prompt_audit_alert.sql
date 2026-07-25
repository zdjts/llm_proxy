-- 0010: Prompt templates, audit trail, shared sessions, and alert infrastructure (T132, T135, T136, T137)
-- Part of v3.0 full-stack upgrade per docs/BRIEF-v3.0-fullstack-upgrade.md

-- Prompt template library
CREATE TABLE IF NOT EXISTS prompt_template (
    id          TEXT PRIMARY KEY,                  -- UUID
    name        TEXT NOT NULL,
    description TEXT,
    team_id     TEXT,                              -- NULL = personal, non-null = shared within team
    owner_id    TEXT NOT NULL REFERENCES user_account(id),
    tags        TEXT,                              -- comma-separated tags
    is_public   INTEGER NOT NULL DEFAULT 0,        -- 1 = visible to entire team/workspace
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE TABLE IF NOT EXISTS prompt_template_version (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    template_id     TEXT NOT NULL REFERENCES prompt_template(id) ON DELETE CASCADE,
    version         INTEGER NOT NULL,
    content         TEXT NOT NULL,                  -- the prompt template text with {{variables}}
    variables_json  TEXT,                           -- JSON schema of expected variables
    notes           TEXT,                           -- version change notes
    created_at      INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    UNIQUE(template_id, version)
);

-- Playground shared sessions
CREATE TABLE IF NOT EXISTS shared_playground_session (
    id          TEXT PRIMARY KEY,                  -- short unique ID for URL sharing
    owner_id    TEXT NOT NULL REFERENCES user_account(id),
    config_json TEXT NOT NULL,                     -- full session config (models, messages, params)
    expires_at  INTEGER,                           -- NULL = never expires, else epoch ms
    is_active   INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

-- Audit trail for all admin operations
CREATE TABLE IF NOT EXISTS audit_trail (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type  TEXT NOT NULL,                     -- 'key.create', 'key.rotate', 'routing.update', 'pipeline.save', etc.
    actor_id    TEXT,                              -- user_account.id who performed the action
    actor_ip    TEXT,
    target_type TEXT NOT NULL,                     -- 'client_key', 'provider', 'route', 'pipeline', 'quota', 'alert_rule', 'user', 'team'
    target_id   TEXT NOT NULL,                     -- identifier of the affected resource
    before_json TEXT,                              -- JSON snapshot before change (NULL = creation)
    after_json  TEXT,                              -- JSON snapshot after change (NULL = deletion)
    metadata    TEXT,                              -- extra info (user agent, reason, etc.)
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE INDEX IF NOT EXISTS idx_audit_trail_time ON audit_trail(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_trail_target ON audit_trail(target_type, target_id);
CREATE INDEX IF NOT EXISTS idx_audit_trail_actor ON audit_trail(actor_id);

-- Alert rules (dynamic, DB-backed)
CREATE TABLE IF NOT EXISTS alert_rule (
    id          TEXT PRIMARY KEY,                  -- UUID
    name        TEXT NOT NULL,
    description TEXT,
    enabled     INTEGER NOT NULL DEFAULT 1,
    severity    TEXT NOT NULL DEFAULT 'p3',         -- p1, p2, p3, p4
    condition_json TEXT NOT NULL,                   -- structured condition (e.g. {"metric":"error_rate","op":"gt","threshold":0.05,"window_secs":300})
    action_json TEXT,                               -- actions: {"notify_channels":["slack","email"],"auto_disable_key":false}
    tenant_filter TEXT,                             -- NULL = all tenants, else comma-separated tenant IDs
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

-- Alert silence rules
CREATE TABLE IF NOT EXISTS alert_silence (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id     TEXT,                              -- if set, silence for a specific alert_rule; NULL = all rules
    match_model TEXT,                              -- NULL = all models
    match_tenant TEXT,                              -- NULL = all tenants
    start_at    INTEGER NOT NULL,
    end_at      INTEGER NOT NULL,
    reason      TEXT,
    created_by  TEXT REFERENCES user_account(id),
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE INDEX IF NOT EXISTS idx_alert_silence_active ON alert_silence(start_at, end_at);

-- On-call schedule
CREATE TABLE IF NOT EXISTS oncall_schedule (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     TEXT NOT NULL REFERENCES user_account(id),
    team_id     TEXT NOT NULL REFERENCES team(id),
    start_at    INTEGER NOT NULL,                  -- epoch ms
    end_at      INTEGER NOT NULL,                  -- epoch ms
    role        TEXT NOT NULL DEFAULT 'primary',    -- 'primary' | 'secondary'
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE INDEX IF NOT EXISTS idx_oncall_time ON oncall_schedule(start_at, end_at);
CREATE INDEX IF NOT EXISTS idx_oncall_user ON oncall_schedule(user_id);

-- UI event log for product analytics
CREATE TABLE IF NOT EXISTS ui_event_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    event_name  TEXT NOT NULL,                     -- e.g. 'strategy_switch', 'pipeline_save', 'key_rotate', 'page_view'
    user_id     TEXT,                              -- NULL if anonymous/unauthenticated
    page        TEXT,
    metadata    TEXT,                              -- JSON: sanitized event payload (no prompt content)
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE INDEX IF NOT EXISTS idx_ui_event_time ON ui_event_log(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ui_event_name ON ui_event_log(event_name);

-- Data retention configuration (per-table TTL settings)
CREATE TABLE IF NOT EXISTS data_retention_policy (
    table_name      TEXT PRIMARY KEY,
    retention_days  INTEGER NOT NULL,
    is_compliance   INTEGER NOT NULL DEFAULT 0,    -- 1 = compliance mode: cannot reduce retention below minimum
    min_retention   INTEGER NOT NULL DEFAULT 30,
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

-- Insert default retention policies
INSERT OR IGNORE INTO data_retention_policy (table_name, retention_days, is_compliance, min_retention)
VALUES
    ('request_log', 90, 0, 30),
    ('audit_hourly', 365, 0, 90),
    ('audit_trail', 730, 1, 365),
    ('ui_event_log', 90, 0, 30),
    ('alert_event', 365, 0, 90);

-- ADR-013 §2.1: persistent alert_event table.
CREATE TABLE IF NOT EXISTS alert_event (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ts              INTEGER NOT NULL,
    type            TEXT    NOT NULL,
    pool_id         TEXT,
    tenant_id       TEXT,
    key_hash        TEXT,
    model           TEXT,
    error_code      TEXT,
    status          INTEGER,
    msg             TEXT,
    payload         TEXT
);
CREATE INDEX IF NOT EXISTS idx_alert_event_ts ON alert_event(ts);
CREATE INDEX IF NOT EXISTS idx_alert_event_tenant ON alert_event(tenant_id);
CREATE INDEX IF NOT EXISTS idx_alert_event_type ON alert_event(type);

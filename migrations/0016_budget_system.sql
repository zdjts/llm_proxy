-- 0016: Organization / Team / Budget system (T178, v4.0 Track I)
-- Implements Org→Team→Key three-level budget inheritance as described in
-- BRIEF-v4.0 §4 and ADR-016.
--
-- NOTE: "client_key" (auth to gateway) is distinct from "upstream key_entry"
-- (key used to call providers). This migration adds budget fields to the
-- existing `team` table (created in 0009_rbac.sql) and creates `organization`
-- and `budget_usage` tables.

-- ── Organization table ──
-- Top-level entity for budget hierarchy.
CREATE TABLE IF NOT EXISTS organization (
    id              TEXT PRIMARY KEY,       -- e.g. "org-default"
    name            TEXT NOT NULL,
    budget_usd      REAL,                    -- NULL = unlimited
    budget_period   TEXT NOT NULL DEFAULT 'monthly',  -- daily/weekly/monthly/unlimited
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

-- ── Extend team table with budget fields ──
-- The `team` table was created in 0009_rbac.sql. We ALTER TABLE to add
-- budget-related columns without recreating.

ALTER TABLE team ADD COLUMN organization_id TEXT REFERENCES organization(id);
ALTER TABLE team ADD COLUMN budget_usd REAL;            -- NULL = unlimited (inherits from org)
ALTER TABLE team ADD COLUMN budget_period TEXT NOT NULL DEFAULT 'monthly';
ALTER TABLE team ADD COLUMN budget_soft_limit REAL;      -- NULL = no soft limit alert
ALTER TABLE team ADD COLUMN budget_action TEXT NOT NULL DEFAULT 'hard_stop';  -- hard_stop/soft_downgrade/alert_only
ALTER TABLE team ADD COLUMN fallback_models TEXT;        -- JSON array of cheaper model names for soft_downgrade

-- ── Extend auth client_keys with team_id ──
-- client_keys are stored in memory (auth_store.rs). This column allows
-- persisting the team → key mapping for the budget system.
-- We add to the existing client_keys table if it exists, or use the in-memory
-- store. For v4.0, we store the team_id alongside the client_key in auth_store.
-- No ALTER needed here since client_keys are primarily in-memory.

-- ── Budget usage tracking ──
-- Records real-time spend aggregated per period.
CREATE TABLE IF NOT EXISTS budget_usage (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    scope_type      TEXT NOT NULL,           -- 'organization' | 'team' | 'key'
    scope_id        TEXT NOT NULL,           -- org/team/key identifier
    period_start    INTEGER NOT NULL,        -- unix timestamp ms
    period_end      INTEGER NOT NULL,        -- unix timestamp ms
    spend_usd       REAL NOT NULL DEFAULT 0.0,
    request_count   INTEGER NOT NULL DEFAULT 0,
    last_updated    INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    UNIQUE(scope_type, scope_id, period_start)
);

CREATE INDEX IF NOT EXISTS idx_budget_usage_scope ON budget_usage(scope_type, scope_id);
CREATE INDEX IF NOT EXISTS idx_budget_usage_period ON budget_usage(period_start);

-- ── tenant_id ↔ team_id compatibility view ──
-- Existing code uses tenant_id throughout (request_log, audit_hourly, pricing).
-- This view maps team_id to tenant_id for backward compatibility (T182).
CREATE VIEW IF NOT EXISTS team_tenant_map AS
SELECT
    t.id AS team_id,
    t.id AS tenant_id,
    t.organization_id
FROM team t;

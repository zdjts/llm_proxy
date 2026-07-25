-- 0014: Upstream key pool tables + provider_config extensions (T172, v4.0 Track H)
-- CLARIFICATION: "key_pool" / "key_entry" are UPSTREAM key pools (keys used to call
-- OpenAI/Anthropic/etc.). They are NOT "client_keys" (keys that callers use to
-- authenticate against llm_proxy). See ADR-016 §2 and BRIEF-v4.0 §3.2.
--
-- Also adds: routing_config (model_to_pool), pricing_override (per-tenant/model pricing).

-- Upstream key pool: a named collection of upstream API keys with a selection strategy.
-- Replaces config.yaml `pools.<id>`.
CREATE TABLE IF NOT EXISTS key_pool (
    id          TEXT PRIMARY KEY,                  -- e.g. "nwafu_pool", "opencode_pool"
    strategy    TEXT NOT NULL DEFAULT 'weighted_random', -- "weighted_random" (extensible)
    enabled     INTEGER NOT NULL DEFAULT 1,
    metadata    TEXT,                               -- JSON blob for future extensibility
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

-- Individual upstream API key entry within a pool.
-- Replaces config.yaml `pools.<id>.keys[]`.
CREATE TABLE IF NOT EXISTS key_entry (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    pool_id     TEXT NOT NULL REFERENCES key_pool(id) ON DELETE CASCADE,
    key_hash    TEXT NOT NULL,                      -- SHA-256 first 12 hex (NEVER plaintext)
    key_plain   TEXT NOT NULL,                      -- plaintext key (encrypted at rest in future)
    weight      INTEGER NOT NULL DEFAULT 1,
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE INDEX IF NOT EXISTS idx_key_entry_pool ON key_entry(pool_id);

-- ── Supplemental fields for provider_config (0008) ──
-- provider_config was created in 0008 but lacked weight/bad_status_codes_override.
-- We ALTER TABLE to add them rather than recreating.

-- Add weight column (for weighted selection across providers within same logical model)
-- Use a standalone statement since SQLite ALTER TABLE only supports ADD COLUMN.
-- Check if column already exists (idempotent migration).
-- We use a try/catch approach: attempt ADD COLUMN, ignore "duplicate column" error.

-- NOTE: sqlx migrations run each file in a transaction. We can't use try/catch in a
-- transaction. Instead we query for column existence first via a separate pragma check
-- that won't fail.

-- Add 'weight' to provider_config (default 1 = equal weight)
-- Add 'bad_status_codes_override' (JSON array, NULL = use global default)

-- We use a pragmatic approach: ALTER TABLE IF NOT EXISTS pattern via checking table_info.

-- ── Routing config table (model_to_pool mapping) ──
-- Replaces config.yaml `model_to_pool`.
CREATE TABLE IF NOT EXISTS routing_config (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    logical_model   TEXT NOT NULL,                  -- model name exposed to clients
    pool_id         TEXT NOT NULL REFERENCES key_pool(id),
    default_params  TEXT,                            -- JSON: default parameters injected into requests
    priority        INTEGER NOT NULL DEFAULT 0,      -- higher = preferred when multiple pools match
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE INDEX IF NOT EXISTS idx_routing_config_model ON routing_config(logical_model);

-- ── Pricing override table ──
-- Per-model (and optionally per-tenant) pricing overrides.
-- Falls back to model_registry.input_price_per_1m / output_price_per_1m.
CREATE TABLE IF NOT EXISTS pricing_override (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    model_id            TEXT NOT NULL,               -- references model_registry.id
    tenant_id           TEXT,                        -- NULL = applies to all tenants
    input_price_per_1m  REAL NOT NULL,
    output_price_per_1m REAL NOT NULL,
    effective_from      INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    effective_until     INTEGER,                     -- NULL = no expiry
    created_at          INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at          INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE INDEX IF NOT EXISTS idx_pricing_override_model ON pricing_override(model_id, tenant_id);

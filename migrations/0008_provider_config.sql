-- 0008: Provider configuration, model registry, and shadow routing tables (T131)
-- Part of v3.0 full-stack upgrade per docs/BRIEF-v3.0-fullstack-upgrade.md

CREATE TABLE IF NOT EXISTS provider_config (
    id          TEXT PRIMARY KEY,          -- unique provider identifier, e.g. "azure-eastus"
    kind        TEXT NOT NULL,            -- ProviderKind: "openai" | "anthropic" | "gemini" | "azure" | "bedrock" | "cohere" | "mistral" | "vllm" | "ollama"
    base_url    TEXT NOT NULL,
    pool_id     TEXT NOT NULL,            -- references config pools
    enabled     INTEGER NOT NULL DEFAULT 1,
    metadata    TEXT,                      -- JSON blob: api_version, region, resource_name, etc.
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE TABLE IF NOT EXISTS model_registry (
    id                  TEXT PRIMARY KEY,          -- e.g. "gpt-4o", "claude-sonnet-4-20250514"
    display_name        TEXT NOT NULL,
    provider_kind       TEXT NOT NULL,             -- owning provider kind
    provider_config_id  TEXT,                      -- nullable: which provider_config, or NULL for non-configured
    supports_vision     INTEGER NOT NULL DEFAULT 0,
    supports_tool_calling INTEGER NOT NULL DEFAULT 0,
    supports_json_mode  INTEGER NOT NULL DEFAULT 0,
    max_context_tokens  INTEGER NOT NULL DEFAULT 4096,
    max_output_tokens   INTEGER NOT NULL DEFAULT 4096,
    input_price_per_1m  REAL,                      -- USD per 1M input tokens
    output_price_per_1m REAL,                      -- USD per 1M output tokens
    capabilities_json   TEXT,                       -- JSON: extended capabilities (modalities, languages, etc.)
    enabled             INTEGER NOT NULL DEFAULT 1,
    created_at          INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at          INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE TABLE IF NOT EXISTS shadow_route (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    logical_model   TEXT NOT NULL,                 -- the model name exposed to clients
    primary_physical TEXT NOT NULL,                 -- primary physical model name
    shadow_physical TEXT NOT NULL,                 -- shadow physical model name
    shadow_ratio    REAL NOT NULL DEFAULT 0.0,     -- 0.0 - 1.0, fraction of traffic to mirror
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE INDEX IF NOT EXISTS idx_model_registry_kind ON model_registry(provider_kind);
CREATE INDEX IF NOT EXISTS idx_shadow_route_logical ON shadow_route(logical_model);

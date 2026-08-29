# Slice 0 — Config / AppState inventory

Read-only map for ADR-018 / implementation-plan-structure-refactor.
YAML field names and `client_keys: [{key, tenant_id}]` are unchanged.

Rule chosen for slice 1: **YAML subsystem structs live in `config` (or are
re-exported from `config` when defined next to feature code).** Runtime
owners follow ADR-017.

## Duplicate / similar names

| Name | Role |
|---|---|
| `health` vs `health_check` | Background key probes vs HTTP `/health*` |
| `auth` vs `auth_store` | Middleware + YAML client keys vs process-local key store |
| `router` vs `router_strategy` | Live key/pool router vs extra routing strategies |
| `runtime.rs` vs `RuntimePolicy` | Router rebuild helper vs YAML-static failover/alerts/cache-max |
| `config.pricing` vs `model_metadata.pricing` | Accounting USD/token vs display USD/million (ADR-017) |

## Config types deserialized from `config.yaml`

| Type | Module | Runtime owner (after boot) |
|---|---|---|
| `Config` | `config/mod.rs` | `AppState.config` — YAML bootstrap snapshot; not hot-reloaded |
| `ServerConfig` | `config/mod.rs` | Bind + body limit at serve time; YAML-only |
| `AuthConfig` | `config/mod.rs` | Seeds `AuthStore` / `AuthState.entries`; YAML-only after seed |
| `ClientKeyEntry` | `auth/mod.rs` (re-export via `config`) | `AuthStore` process-local; not restored from DB |
| `DbConfig` | `config/mod.rs` | `sqlx::SqlitePool` in `AppState.db` |
| `FailoverConfig` | `config/mod.rs` | Copied into `RuntimePolicy` + health task; startup-static |
| `PoolConfig` / `KeyEntry` / `PoolStrategy` | `config/mod.rs` | YAML bootstrap → `ConfigStore` SQLite; live `Router` snapshot |
| `ProviderConfig` / `ProviderKind` | `config/mod.rs` | Same as pools: ConfigStore + `ProviderRegistry` |
| `ModelRouting` | `config/mod.rs` | ConfigStore `routing_config` → live `Router` |
| `ModelRegistryConfig` | `config/mod.rs` | Import/export only; `model_registry` table; not callable routes |
| `ModelMetadataConfig` (+ partials) | `config/mod.rs` | ConfigStore snapshot; catalog display only |
| `BootstrapAdminConfig` | `config/mod.rs` | One-shot `rbac::store::bootstrap_admin` |
| `AdminConfig` | `config/mod.rs` | Admin IP guard in `server`; YAML-only |
| `pricing::PricingConfig` | `config/pricing.rs` | ConfigStore accounting carrier; not metadata pricing |
| `RateLimitConfig` | `config/mod.rs` | `RateLimiter` at boot; YAML-only |
| `cache_max_entries` | `Config` field | `PromptCache` + `RuntimePolicy`; YAML-only |
| `AlertConfig` / channels | `config/mod.rs` | Alert task + `RuntimePolicy`; YAML-only |
| `AclConfig` | `auth/acl.rs` (re-export `config`) | Auth ACL; YAML-only |
| `FallbackConfig` | `fallback.rs` (re-export `config`) | `AppState.fallback_config`; YAML-only |
| `ConcurrencyConfig` | `config/mod.rs` | `ConcurrencyLimiter`; YAML-only |

## `AppState` / boot handles (`main` / bootstrap)

| Handle | Constructed from | Owner |
|---|---|---|
| `RouterHandle` | `runtime::rebuild_router(ConfigStore)` | Live callable models (ADR-017) |
| `ModelCatalog` | router + ConfigStore | `/v1/models` projection |
| `SqlitePool` | `config.db.path` | Logs, aggregator, admin, ConfigStore |
| `Arc<Config>` | YAML file | Startup-static policy |
| `PromptCache` | `cache_max_entries` | Process-local cache |
| `Metrics` | default | Prometheus `/metrics` |
| `CircuitBreaker` | defaults | Request path |
| `ConcurrencyLimiter` | YAML concurrency | Request path |
| `FallbackConfig` | YAML | Request path |
| `alert_tx` / `alert_snapshot` | boot | Alerts |
| `AuthStore` | YAML `client_keys` | Auth (ephemeral CRUD) |
| `QuotaTracker` | boot defaults | Middleware |
| `Pipeline` | default config | Transforms |
| `RbacState` | DB + JWT env | Admin API |
| `ConfigStore` | SQLite + YAML seed if empty | Pools/providers/routing |
| `BudgetManager` | DB | Spend tracking |
| `BadKeyRegistry` | empty | Health + router |
| `ProviderRegistry` | factories in bootstrap | Provider trait objects |
| `RateLimiter` | YAML rate_limit | Middleware (not on AppState) |

## Authority (ADR-017, do not collapse)

1. YAML = bootstrap/static (auth, rate limit, concurrency, fallback, pipeline, cache policy, failover/alerts via `RuntimePolicy`).
2. `ConfigStore` = runtime pools/providers/routing (+ bootstrap pricing/metadata copies).
3. Live `Router` = only callable models.
4. Accounting pricing ≠ metadata display pricing.
5. `AuthStore` = YAML seed + process-local; no plaintext in logs.

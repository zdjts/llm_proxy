//! DB-backed configuration store.
//!
//! SQLite is the authoritative source for managed runtime configuration. YAML is
//! accepted only through the explicit validation/import pipeline; startup and the
//! background poller never write YAML values to the database.
//!
//! # Terminology (per ADR-016)
//!
//! - **client_key**: key used by callers to authenticate against llm_proxy.
//! - **upstream key pool entry**: key used by llm_proxy to call upstream
//!   providers (OpenAI, Anthropic, etc.). The `key_pool`/`key_entry` DB
//!   tables store **upstream** keys, NOT client keys.

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tracing;

use crate::config::{
    AlertConfig, CredentialType, FailoverConfig, KeyEntry, ModelMetadataConfig, ModelRouting,
    PoolConfig, ProviderConfig, ProviderKind, ResponseNormalization,
};
use crate::error::AppError;

pub async fn insert_key_entry(
    conn: &mut sqlx::SqliteConnection,
    pool_id: &str,
    key_entry: &KeyEntry,
) -> Result<(), AppError> {
    let kh = key_entry.identity_hash();
    sqlx::query(
        "INSERT INTO key_entry (pool_id, key_hash, key_plain, weight, enabled, cred_type, refresh_token, expires_at, issuer) \
         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8)",
    )
    .bind(pool_id)
    .bind(&kh)
    .bind(&key_entry.key)
    .bind(key_entry.weight as i64)
    .bind(key_entry.cred_type_str())
    .bind(key_entry.refresh.as_deref())
    .bind(key_entry.expires)
    .bind(key_entry.issuer.as_deref())
    .execute(conn)
    .await
    .map_err(|e| AppError::Internal(format!("ConfigStore import key: {e}")))?;
    Ok(())
}

/// Current wall-clock time in epoch **milliseconds**, matching the units of
/// `pricing_override.effective_from` / `effective_until`.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Build the effective accounting price table by layering four sources.
///
/// Later layers win over earlier ones (highest priority first):
///
/// 1. `pricing_override` row for this exact (model, tenant)
/// 2. `pricing_override` row for this model with `tenant_id IS NULL`
/// 3. the startup YAML `pricing:` snapshot (`bootstrap`)
/// 4. `model_registry` catalog price (`import-models` writes these)
///
/// Layers 1–2 are DB-owned and hot-reloadable, which is why they outrank YAML
/// (ADR-017: SQLite is authoritative for managed configuration). Layer 4 is
/// last because ADR-017 forbids catalog/metadata prices from overriding an
/// explicit accounting price — it is only the fallback that keeps costs from
/// being silently zero for models nobody priced by hand.
///
/// A model is emitted only if some layer produced a price; otherwise the
/// lookup falls through to `PricingConfig`'s zero entry and `compute_cost`
/// records `NULL`, which is the pre-existing behaviour for unpriced models.
fn compose_pricing(
    bootstrap: &crate::config::pricing::PricingConfig,
    catalog: &[ModelRegistryEntry],
    overrides: &[PricingOverride],
    now_ms: i64,
) -> crate::config::pricing::PricingConfig {
    use crate::config::pricing::{ModelPricing, PriceEntry};

    let mut models: HashMap<String, ModelPricing> = HashMap::new();

    // Layer 4 — catalog defaults.
    for entry in catalog {
        let price = PriceEntry {
            prompt: entry.input_price_per_1m.unwrap_or(0.0),
            completion: entry.output_price_per_1m.unwrap_or(0.0),
        };
        models.insert(
            entry.id.clone(),
            ModelPricing {
                default: price,
                tenants: HashMap::new(),
            },
        );
    }

    // Layer 3 — YAML `pricing:` overrides the catalog per model, and
    // contributes the per-tenant entries the catalog cannot express.
    for (model, yaml_pricing) in &bootstrap.models {
        let slot = models.entry(model.clone()).or_insert_with(|| ModelPricing {
            default: PriceEntry::default(),
            tenants: HashMap::new(),
        });
        if yaml_pricing.default.prompt > 0.0 || yaml_pricing.default.completion > 0.0 {
            slot.default = yaml_pricing.default.clone();
        }
        for (tenant, price) in &yaml_pricing.tenants {
            slot.tenants.insert(tenant.clone(), price.clone());
        }
    }

    // Layers 1–2 — DB overrides. Two passes so a tenant-specific row always
    // wins over the global row regardless of insertion order.
    for ov in overrides.iter().filter(|o| o.is_effective_at(now_ms)) {
        if ov.tenant_id.is_some() {
            continue;
        }
        let slot = models
            .entry(ov.model_id.clone())
            .or_insert_with(|| ModelPricing {
                default: PriceEntry::default(),
                tenants: HashMap::new(),
            });
        slot.default = PriceEntry {
            prompt: ov.input_price_per_1m,
            completion: ov.output_price_per_1m,
        };
    }
    for ov in overrides
        .iter()
        .filter(|o| o.is_effective_at(now_ms) && o.tenant_id.is_some())
    {
        let tenant = match ov.tenant_id.as_deref() {
            Some(t) => t,
            None => continue,
        };
        let slot = models
            .entry(ov.model_id.clone())
            .or_insert_with(|| ModelPricing {
                default: PriceEntry::default(),
                tenants: HashMap::new(),
            });
        slot.tenants.insert(
            tenant.to_owned(),
            PriceEntry {
                prompt: ov.input_price_per_1m,
                completion: ov.output_price_per_1m,
            },
        );
    }

    crate::config::pricing::PricingConfig { models }
}

fn parse_cred_type(raw: Option<&str>) -> CredentialType {
    match raw {
        Some(s) if s.eq_ignore_ascii_case("oauth") => CredentialType::Oauth,
        _ => CredentialType::ApiKey,
    }
}

// ── In-memory config snapshots ──

/// A row of `pricing_override` — the DB-owned accounting price for one
/// (model, tenant) pair, optionally bounded by a validity window.
///
/// `effective_from` / `effective_until` are epoch **milliseconds** (the table
/// defaults to `unixepoch('subsec') * 1000`).
#[derive(Debug, Clone)]
pub struct PricingOverride {
    pub model_id: String,
    /// `None` means "applies to every tenant".
    pub tenant_id: Option<String>,
    pub input_price_per_1m: f64,
    pub output_price_per_1m: f64,
    pub effective_from: i64,
    pub effective_until: Option<i64>,
}

impl PricingOverride {
    /// Whether this row is in force at `now_ms`.
    pub fn is_effective_at(&self, now_ms: i64) -> bool {
        if now_ms < self.effective_from {
            return false;
        }
        match self.effective_until {
            None => true,
            Some(until) => now_ms < until,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelRegistryEntry {
    pub id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub provider_config_id: Option<String>,
    pub supports_vision: bool,
    pub supports_tool_calling: bool,
    pub supports_json_mode: bool,
    pub max_context_tokens: i32,
    pub max_output_tokens: i32,
    pub input_price_per_1m: Option<f64>,
    pub output_price_per_1m: Option<f64>,
    pub capabilities_json: Option<String>,
    pub enabled: bool,
}

/// Startup-static policy copied from YAML (ADR-017). Not hot-reloaded.
#[derive(Debug, Clone)]
pub struct RuntimePolicy {
    pub failover: FailoverConfig,
    pub alerts: AlertConfig,
    pub cache_max_entries: usize,
    pub response_normalization: ResponseNormalization,
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self {
            failover: FailoverConfig {
                enabled: true,
                bad_status_codes: vec![401, 402, 403, 429],
                max_retries: 1,
                probe_interval_secs: 60,
                probe_timeout_secs: 10,
                max_probe_retries: 3,
            },
            alerts: AlertConfig::default(),
            cache_max_entries: 256,
            response_normalization: ResponseNormalization::default(),
        }
    }
}

/// Every field is behind `Arc` so readers get a consistent view without
/// holding a lock across `.await`.
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    pub providers: Arc<Vec<ProviderConfig>>,
    pub pool_configs: Arc<HashMap<String, PoolConfig>>,
    pub model_routing: Arc<HashMap<String, ModelRouting>>,
    pub model_registry: Arc<Vec<ModelRegistryEntry>>,
    pub model_metadata: Arc<ModelMetadataConfig>,
    /// The **composed** accounting price table: DB overrides over the YAML
    /// base layer over the `model_registry` catalog price. This is what
    /// request accounting and the dashboard cost views read.
    pub pricing: Arc<crate::config::pricing::PricingConfig>,
    /// Layer 3 of the pricing stack: the startup YAML `pricing:` snapshot.
    /// Kept separately because [`ConfigStore::refresh_from_db`] must recompose
    /// `pricing` without losing it (ADR-017: YAML is a base layer, not the
    /// authority, for managed configuration).
    bootstrap_pricing: Arc<crate::config::pricing::PricingConfig>,
    pub runtime: Arc<RuntimePolicy>,
    pub version: u64,
}

impl Default for ConfigSnapshot {
    fn default() -> Self {
        Self {
            providers: Arc::new(Vec::new()),
            pool_configs: Arc::new(HashMap::new()),
            model_routing: Arc::new(HashMap::new()),
            model_registry: Arc::new(Vec::new()),
            model_metadata: Arc::new(ModelMetadataConfig::default()),
            pricing: Arc::new(crate::config::pricing::PricingConfig::default()),
            bootstrap_pricing: Arc::new(crate::config::pricing::PricingConfig::default()),
            runtime: Arc::new(RuntimePolicy::default()),
            version: 0,
        }
    }
}

/// Central store for DB-managed configuration.
///
/// All public methods return clones of `Arc`-wrapped data, so callers
/// never hold the lock across I/O.
pub struct ConfigStore {
    inner: RwLock<ConfigSnapshot>,
    db: SqlitePool,
}

impl ConfigStore {
    /// Construct an in-memory store for unit and HTTP fixture tests.
    pub fn for_test(db: SqlitePool, model_metadata: ModelMetadataConfig) -> Self {
        Self {
            inner: RwLock::new(ConfigSnapshot {
                model_metadata: Arc::new(model_metadata),
                ..Default::default()
            }),
            db,
        }
    }

    /// Install the startup YAML `pricing:` snapshot as the **base layer** and
    /// recompose the effective price table.
    ///
    /// This does not overwrite DB-owned overrides: [`Self::refresh_from_db`]
    /// keeps `bootstrap_pricing` around precisely so a refresh cannot drop it.
    pub async fn set_bootstrap_pricing(&self, pricing: crate::config::pricing::PricingConfig) {
        self.inner.write().await.bootstrap_pricing = Arc::new(pricing);
        if let Err(e) = self.refresh_pricing().await {
            // A pricing load failure must not take the gateway down; the
            // previously composed table stays in force.
            tracing::error!(error = %e, "ConfigStore: pricing recompose failed");
        }
    }

    pub async fn pricing(&self) -> Arc<crate::config::pricing::PricingConfig> {
        self.inner.read().await.pricing.clone()
    }

    pub async fn set_bootstrap_model_metadata(&self, metadata: ModelMetadataConfig) {
        self.inner.write().await.model_metadata = Arc::new(metadata);
    }

    pub async fn set_bootstrap_runtime(&self, runtime: RuntimePolicy) {
        self.inner.write().await.runtime = Arc::new(runtime);
    }

    pub async fn runtime(&self) -> Arc<RuntimePolicy> {
        self.inner.read().await.runtime.clone()
    }

    /// Convenience accessor for the normalisation policy that lives
    /// inside [`RuntimePolicy`].  Kept as a method so handler code does
    /// not have to walk the runtime snapshot itself; the snapshot is
    /// still the authoritative carrier.
    pub async fn response_normalization(&self) -> crate::config::ResponseNormalization {
        self.inner
            .read()
            .await
            .runtime
            .response_normalization
            .clone()
    }

    ///
    /// Restarting the process must not overwrite administrator changes in the
    /// database with values from a YAML file.
    pub async fn load(db: SqlitePool) -> Result<Self, AppError> {
        let store = Self {
            inner: RwLock::new(ConfigSnapshot::default()),
            db,
        };
        store.refresh_from_db().await?;
        Ok(store)
    }

    /// Parse and validate a YAML document without changing active state.
    pub fn validate_yaml(yaml: &str) -> Result<crate::config::Config, AppError> {
        let config: crate::config::Config = serde_yaml::from_str(yaml)
            .map_err(|e| AppError::Config(format!("Failed to parse import document: {e}")))?;
        config.validate()?;
        Ok(config)
    }

    /// Import a YAML document atomically, refreshing the active snapshot only
    /// after the database transaction has committed.
    pub async fn import_yaml(&self, yaml: &str) -> Result<(), AppError> {
        let config = Self::validate_yaml(yaml)?;
        let mut tx = self.db.begin().await.map_err(|e| {
            AppError::Internal(format!("ConfigStore begin import transaction: {e}"))
        })?;

        // Full-document replace for managed sections so export→import between
        // hosts converges instead of leaving stale pools/providers/routes.
        // model_registry is catalog-owned (models.dev); export omits it and
        // import only upserts if the YAML still carries the optional section.
        // Order matters: routing_config.pool_id FK → key_pool; key_entry cascades.
        sqlx::query("DELETE FROM routing_config")
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("ConfigStore clear routing: {e}")))?;
        sqlx::query("DELETE FROM key_entry")
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("ConfigStore clear keys: {e}")))?;
        sqlx::query("DELETE FROM key_pool")
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("ConfigStore clear pools: {e}")))?;
        sqlx::query("DELETE FROM provider_config")
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("ConfigStore clear providers: {e}")))?;

        for (pool_id, pool_cfg) in &config.pools {
            sqlx::query("INSERT INTO key_pool (id, strategy, enabled) VALUES (?1, ?2, 1)")
                .bind(pool_id)
                .bind("weighted_random")
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::Internal(format!("ConfigStore import pool: {e}")))?;
            for key_entry in &pool_cfg.keys {
                insert_key_entry(tx.as_mut(), pool_id, key_entry).await?;
            }
        }
        for provider in &config.providers {
            let mut metadata = provider.metadata.clone();
            if !metadata.is_object() {
                metadata = serde_json::json!({});
            }
            sqlx::query("INSERT INTO provider_config (id, kind, base_url, pool_id, enabled, metadata) VALUES (?1, ?2, ?3, ?4, 1, ?5)")
                .bind(&provider.id).bind(provider_kind_to_str(&provider.kind)).bind(&provider.base_url)
                .bind(&provider.pool_id).bind(serde_json::to_string(&metadata).unwrap_or_default())
                .execute(&mut *tx).await.map_err(|e| AppError::Internal(format!("ConfigStore import provider: {e}")))?;
        }
        for (model, routing) in &config.model_to_pool {
            let params = routing
                .default_params()
                .map(|v| serde_json::to_string(v).unwrap_or_default());
            sqlx::query("INSERT INTO routing_config (logical_model, pool_id, default_params, upstream_model, enabled) VALUES (?1, ?2, ?3, ?4, 1)")
                .bind(model).bind(routing.pool_id()).bind(params.as_deref()).bind(routing.upstream_model()).execute(&mut *tx).await
                .map_err(|e| AppError::Internal(format!("ConfigStore import routing: {e}")))?;
        }
        for entry in &config.model_registry {
            let caps = if entry.capabilities_json.is_null() {
                None
            } else {
                Some(
                    serde_json::to_string(&entry.capabilities_json).map_err(|e| {
                        AppError::Config(format!(
                            "model_registry entry '{}' capabilities_json serialize: {e}",
                            entry.id
                        ))
                    })?,
                )
            };
            sqlx::query(
                "INSERT INTO model_registry (
                    id, display_name, provider_kind, provider_config_id,
                    supports_vision, supports_tool_calling, supports_json_mode,
                    max_context_tokens, max_output_tokens,
                    input_price_per_1m, output_price_per_1m,
                    capabilities_json, enabled
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                ON CONFLICT(id) DO UPDATE SET
                    display_name = excluded.display_name,
                    provider_kind = excluded.provider_kind,
                    provider_config_id = excluded.provider_config_id,
                    supports_vision = excluded.supports_vision,
                    supports_tool_calling = excluded.supports_tool_calling,
                    supports_json_mode = excluded.supports_json_mode,
                    max_context_tokens = excluded.max_context_tokens,
                    max_output_tokens = excluded.max_output_tokens,
                    input_price_per_1m = excluded.input_price_per_1m,
                    output_price_per_1m = excluded.output_price_per_1m,
                    capabilities_json = excluded.capabilities_json,
                    enabled = excluded.enabled,
                    updated_at = unixepoch('subsec') * 1000",
            )
            .bind(&entry.id)
            .bind(&entry.display_name)
            .bind(&entry.provider_kind)
            .bind(entry.provider_config_id.as_deref())
            .bind(entry.supports_vision as i32)
            .bind(entry.supports_tool_calling as i32)
            .bind(entry.supports_json_mode as i32)
            .bind(entry.max_context_tokens)
            .bind(entry.max_output_tokens)
            .bind(entry.input_price_per_1m)
            .bind(entry.output_price_per_1m)
            .bind(caps.as_deref())
            .bind(entry.enabled as i32)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("ConfigStore import model_registry: {e}")))?;
        }
        tx.commit()
            .await
            .map_err(|e| AppError::Internal(format!("ConfigStore commit import: {e}")))?;
        self.refresh_from_db().await
    }

    /// Refresh the in-memory cache from DB tables.
    ///
    /// Reads all managed tables and atomically swaps the cache.
    pub async fn refresh_from_db(&self) -> Result<(), AppError> {
        let providers = self.load_providers_from_db().await?;
        let pool_configs = self.load_pools_from_db().await?;
        let model_routing = self.load_routing_from_db().await?;
        let model_registry = self.load_model_registry_from_db().await?;
        let pricing_overrides = self.load_pricing_overrides_from_db().await?;

        let mut current = self.inner.write().await;
        current.providers = Arc::new(providers);
        current.pool_configs = Arc::new(pool_configs);
        current.model_routing = Arc::new(model_routing);
        current.model_registry = Arc::new(model_registry);
        // Layer 4 (catalog) was just reloaded; recompose the price table so a
        // registry price edit takes effect without a restart.
        current.pricing = Arc::new(compose_pricing(
            &current.bootstrap_pricing,
            &current.model_registry,
            &pricing_overrides,
            now_ms(),
        ));
        current.version += 1;

        let version = current.version;
        drop(current);

        tracing::debug!(version, "ConfigStore: cache refreshed");
        Ok(())
    }

    /// Recompose the effective price table without reloading other sections.
    ///
    /// Used by [`Self::set_bootstrap_pricing`], which installs the YAML base
    /// layer at startup — after `load()` has already populated the catalog.
    async fn refresh_pricing(&self) -> Result<(), AppError> {
        let model_registry = self.load_model_registry_from_db().await?;
        let pricing_overrides = self.load_pricing_overrides_from_db().await?;

        let mut current = self.inner.write().await;
        current.model_registry = Arc::new(model_registry);
        current.pricing = Arc::new(compose_pricing(
            &current.bootstrap_pricing,
            &current.model_registry,
            &pricing_overrides,
            now_ms(),
        ));
        Ok(())
    }

    /// Load every `pricing_override` row, including expired ones.
    ///
    /// Validity windows are evaluated in [`compose_pricing`] against a single
    /// consistent `now`, so a mid-load clock tick cannot split a window.
    async fn load_pricing_overrides_from_db(&self) -> Result<Vec<PricingOverride>, AppError> {
        #[derive(sqlx::FromRow)]
        struct DbOverride {
            model_id: String,
            tenant_id: Option<String>,
            input_price_per_1m: f64,
            output_price_per_1m: f64,
            effective_from: i64,
            effective_until: Option<i64>,
        }

        let rows: Vec<DbOverride> = sqlx::query_as(
            "SELECT model_id, tenant_id, input_price_per_1m, output_price_per_1m, \
             effective_from, effective_until \
             FROM pricing_override",
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("ConfigStore load pricing_override: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| PricingOverride {
                model_id: r.model_id,
                tenant_id: r.tenant_id,
                input_price_per_1m: r.input_price_per_1m,
                output_price_per_1m: r.output_price_per_1m,
                effective_from: r.effective_from,
                effective_until: r.effective_until,
            })
            .collect())
    }

    async fn load_providers_from_db(&self) -> Result<Vec<ProviderConfig>, AppError> {
        #[derive(sqlx::FromRow)]
        #[allow(dead_code)]
        struct DbProvider {
            id: String,
            kind: String,
            base_url: String,
            pool_id: String,
            metadata: Option<String>,
            weight: Option<i64>,
            bad_status_codes_override: Option<String>,
        }

        let rows: Vec<DbProvider> = sqlx::query_as(
            "SELECT id, kind, base_url, pool_id, metadata, weight, bad_status_codes_override \
             FROM provider_config WHERE enabled = 1",
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("ConfigStore load providers: {e}")))?;

        rows.into_iter()
            .map(|r| {
                let metadata = r
                    .metadata
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                Ok(ProviderConfig {
                    id: r.id,
                    kind: str_to_provider_kind(&r.kind)?,
                    base_url: r.base_url,
                    pool_id: r.pool_id,
                    metadata,
                })
            })
            .collect()
    }

    async fn load_pools_from_db(&self) -> Result<HashMap<String, PoolConfig>, AppError> {
        #[derive(sqlx::FromRow)]
        struct DbKeyEntry {
            pool_id: String,
            key_hash: String,
            key_plain: String,
            weight: i64,
            cred_type: Option<String>,
            refresh_token: Option<String>,
            expires_at: Option<i64>,
            issuer: Option<String>,
        }

        // Load pools
        #[derive(sqlx::FromRow)]
        struct DbPool {
            id: String,
        }

        let db_pools: Vec<DbPool> = sqlx::query_as("SELECT id FROM key_pool WHERE enabled = 1")
            .fetch_all(&self.db)
            .await
            .map_err(|e| AppError::Internal(format!("ConfigStore load pools: {e}")))?;

        // Load keys
        let db_keys: Vec<DbKeyEntry> = sqlx::query_as(
            "SELECT pool_id, key_hash, key_plain, weight, cred_type, refresh_token, expires_at, issuer \
             FROM key_entry WHERE enabled = 1",
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("ConfigStore load keys: {e}")))?;

        // Group keys by pool_id
        let mut keys_by_pool: HashMap<String, Vec<KeyEntry>> = HashMap::new();
        for k in db_keys {
            keys_by_pool.entry(k.pool_id).or_default().push(KeyEntry {
                key: k.key_plain,
                weight: k.weight as u32,
                cred_type: parse_cred_type(k.cred_type.as_deref()),
                refresh: k.refresh_token,
                expires: k.expires_at,
                issuer: k.issuer,
                identity: Some(k.key_hash),
            });
        }

        let mut pools = HashMap::new();
        for p in db_pools {
            let keys = keys_by_pool.remove(&p.id).unwrap_or_default();
            pools.insert(p.id.clone(), PoolConfig { keys });
        }

        Ok(pools)
    }

    async fn load_routing_from_db(&self) -> Result<HashMap<String, ModelRouting>, AppError> {
        #[derive(sqlx::FromRow)]
        struct DbRouting {
            logical_model: String,
            pool_id: String,
            default_params: Option<String>,
            upstream_model: Option<String>,
        }

        let rows: Vec<DbRouting> = sqlx::query_as(
            "SELECT logical_model, pool_id, default_params, upstream_model FROM routing_config WHERE enabled = 1",
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("ConfigStore load routing: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let default_params = r
                    .default_params
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(serde_json::Value::Null);
                let upstream_model = r
                    .upstream_model
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty());
                let routing = if default_params.is_object() || upstream_model.is_some() {
                    ModelRouting::WithParams {
                        pool: r.pool_id,
                        default_params,
                        upstream_model,
                    }
                } else {
                    ModelRouting::Simple(r.pool_id)
                };
                (r.logical_model, routing)
            })
            .collect())
    }

    /// AUDIT-14 Fix: Load model_registry table from DB.
    async fn load_model_registry_from_db(&self) -> Result<Vec<ModelRegistryEntry>, AppError> {
        #[derive(sqlx::FromRow)]
        struct ModelRow {
            id: String,
            display_name: String,
            provider_kind: String,
            provider_config_id: Option<String>,
            supports_vision: i32,
            supports_tool_calling: i32,
            supports_json_mode: i32,
            max_context_tokens: i32,
            max_output_tokens: i32,
            input_price_per_1m: Option<f64>,
            output_price_per_1m: Option<f64>,
            capabilities_json: Option<String>,
            enabled: i32,
        }

        let rows: Vec<ModelRow> = sqlx::query_as(
            "SELECT id, display_name, provider_kind, provider_config_id, \
             supports_vision, supports_tool_calling, supports_json_mode, \
             max_context_tokens, max_output_tokens, input_price_per_1m, output_price_per_1m, capabilities_json, enabled \
             FROM model_registry",
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("ConfigStore load model_registry: {e}")))?;

        let result: Vec<ModelRegistryEntry> = rows
            .into_iter()
            .map(|r| ModelRegistryEntry {
                id: r.id,
                display_name: r.display_name,
                provider_kind: r.provider_kind,
                provider_config_id: r.provider_config_id,
                supports_vision: r.supports_vision != 0,
                supports_tool_calling: r.supports_tool_calling != 0,
                supports_json_mode: r.supports_json_mode != 0,
                max_context_tokens: r.max_context_tokens,
                max_output_tokens: r.max_output_tokens,
                input_price_per_1m: r.input_price_per_1m,
                output_price_per_1m: r.output_price_per_1m,
                capabilities_json: r.capabilities_json,
                enabled: r.enabled != 0,
            })
            .collect();
        Ok(result)
    }

    // ── Public accessors (return Arc clones, no lock held across .await) ──

    pub async fn snapshot(&self) -> ConfigSnapshot {
        self.inner.read().await.clone()
    }

    /// Replace the static YAML metadata projection after a successful reload.
    pub async fn update_model_metadata(&self, metadata: ModelMetadataConfig) {
        self.inner.write().await.model_metadata = Arc::new(metadata);
    }
    /// Get the current config version (monotonically increasing).
    pub async fn version(&self) -> u64 {
        self.inner.read().await.version
    }

    /// Get a reference to the DB pool for admin API writes.
    pub fn db(&self) -> &SqlitePool {
        &self.db
    }
}

// ── Hot-reload poller ──

/// Spawn a background task that polls the DB for config changes and
/// refreshes the ConfigStore cache.
pub fn spawn_config_poller(
    store: Arc<ConfigStore>,
    interval_secs: u64,
    mut shutdown: tokio::sync::watch::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        // Skip immediate tick (config was just loaded)
        interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match store.refresh_from_db().await {
                        Ok(()) => {} // version bump logged inside refresh
                        Err(e) => {
                            tracing::warn!(error = %e, "ConfigStore poller: refresh failed, keeping previous cache");
                        }
                    }
                }
                _ = shutdown.changed() => {
                    tracing::info!("ConfigStore poller: shutting down");
                    return;
                }
            }
        }
    })
}

// ── Helpers ──

fn provider_kind_to_str(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAi => "openai",
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::Gemini => "gemini",
    }
}

fn str_to_provider_kind(s: &str) -> Result<ProviderKind, AppError> {
    match s {
        "openai" | "open_ai" => Ok(ProviderKind::OpenAi),
        "anthropic" => Ok(ProviderKind::Anthropic),
        "gemini" => Ok(ProviderKind::Gemini),
        other => Err(AppError::Config(format!(
            "Unsupported provider kind '{other}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_test_db() -> (SqlitePool, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_config.db");
        let path_str = db_path.to_str().unwrap();

        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path_str)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

        let pool = SqlitePool::connect_with(options).await.unwrap();

        // Run migrations
        let migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
            .await
            .unwrap();
        migrator.run(&pool).await.unwrap();

        (pool, dir)
    }

    fn test_yaml_document() -> &'static str {
        "server:\n  host: 127.0.0.1\n  port: 4000\nauth:\n  client_keys: []\ndb:\n  path: ./test.db\nfailover:\n  enabled: true\npools:\n  test_pool:\n    keys:\n      - key: sk-test-abc\n        weight: 1\nproviders:\n  - id: test_provider\n    pool_id: test_pool\n    base_url: https://api.test.com/v1\nmodel_to_pool:\n  test-model: test_pool\n"
    }

    #[tokio::test]
    async fn model_metadata_updates_in_store_snapshot() {
        let db = sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        let store = ConfigStore::for_test(db, ModelMetadataConfig::default());
        let mut metadata = ModelMetadataConfig::default();
        metadata.defaults.context_window = Some(321);
        store.update_model_metadata(metadata).await;
        assert_eq!(
            store
                .snapshot()
                .await
                .model_metadata
                .defaults
                .context_window,
            Some(321)
        );
    }
    #[tokio::test]
    async fn import_yaml_replaces_managed_sections_instead_of_merging() {
        let (pool, _dir) = setup_test_db().await;
        let store = ConfigStore::load(pool.clone()).await.unwrap();
        store.import_yaml(test_yaml_document()).await.unwrap();

        let replacement = "server:\n  host: 127.0.0.1\n  port: 4000\nauth:\n  client_keys: []\ndb:\n  path: ./test.db\nfailover:\n  enabled: true\npools:\n  only_pool:\n    keys:\n      - key: sk-only\n        weight: 1\nproviders:\n  - id: only_provider\n    kind: open_ai\n    pool_id: only_pool\n    base_url: https://only.example/v1\nmodel_to_pool:\n  only-model: only_pool\n";
        store.import_yaml(replacement).await.unwrap();

        let snap = store.snapshot().await;
        assert_eq!(snap.pool_configs.len(), 1);
        assert!(snap.pool_configs.contains_key("only_pool"));
        assert!(!snap.pool_configs.contains_key("test_pool"));
        assert_eq!(snap.providers.len(), 1);
        assert_eq!(snap.providers[0].id, "only_provider");
        assert_eq!(snap.providers[0].kind, ProviderKind::OpenAi);
        assert_eq!(snap.model_routing.len(), 1);
        assert!(snap.model_routing.contains_key("only-model"));
        assert!(!snap.model_routing.contains_key("test-model"));
    }

    #[tokio::test]
    async fn invalid_yaml_import_keeps_database_and_snapshot_unchanged() {
        let (pool, _dir) = setup_test_db().await;
        let store = ConfigStore::load(pool.clone()).await.unwrap();
        store.import_yaml(test_yaml_document()).await.unwrap();
        let before_version = store.version().await;
        let before = store.snapshot().await;
        let result = store.import_yaml("pools: [not-a-map]").await;
        assert!(result.is_err());
        assert_eq!(store.version().await, before_version);
        assert_eq!(
            store.snapshot().await.providers.len(),
            before.providers.len()
        );
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM provider_config")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, before.providers.len() as i64);
    }
    #[tokio::test]
    async fn it_refreshes_from_db_after_write() {
        let (pool, _dir) = setup_test_db().await;
        let store = Arc::new(ConfigStore::load(pool.clone()).await.unwrap());
        store.import_yaml(test_yaml_document()).await.unwrap();
        sqlx::query(
            "INSERT INTO provider_config (id, kind, base_url, pool_id, enabled) \
             VALUES ('new_provider', 'anthropic', 'https://api.anthropic.com/v1', 'test_pool', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Refresh
        store.refresh_from_db().await.unwrap();
        let providers = store.snapshot().await.providers;
        assert_eq!(providers.len(), 2);
    }

    #[tokio::test]
    async fn it_handles_refresh_with_no_changes() {
        let (pool, _dir) = setup_test_db().await;
        let store = ConfigStore::load(pool).await.unwrap();
        let v1 = store.version().await;

        store.refresh_from_db().await.unwrap();
        let v2 = store.version().await;
        assert!(
            v2 > v1,
            "version should increment even on no-change refresh"
        );
    }

    // ── Pricing layering (ADR-017: DB-owned accounting prices) ───────────
    //
    // The four layers, highest priority first:
    //   1. pricing_override for (model, tenant)
    //   2. pricing_override for model with tenant_id IS NULL
    //   3. startup YAML `pricing:` snapshot
    //   4. model_registry catalog price (written by import-models)
    // Layer 4 matters because it is what makes cost non-zero for models
    // nobody priced by hand — the exact bug that left request_log.cost_usd
    // NULL for every row.

    fn pricing_yaml_with_model() -> crate::config::pricing::PricingConfig {
        serde_yaml::from_str("yaml-model:\n  prompt: 5.0\n  completion: 20.0")
            .expect("yaml pricing fixture parses")
    }

    async fn insert_catalog_model(pool: &SqlitePool, id: &str, input: f64, output: f64) {
        sqlx::query(
            "INSERT INTO model_registry (id, display_name, provider_kind, \
             max_context_tokens, max_output_tokens, \
             input_price_per_1m, output_price_per_1m, enabled) \
             VALUES (?1, ?1, 'openai', 1000, 1000, ?2, ?3, 1)",
        )
        .bind(id)
        .bind(input)
        .bind(output)
        .execute(pool)
        .await
        .expect("catalog model insert");
    }

    #[tokio::test]
    async fn pricing_layer4_catalog_fills_pricing_carrier() {
        // The regression this locks in: with no YAML `pricing:` section and no
        // overrides, a model priced only in model_registry must still resolve
        // to a non-zero accounting price, so compute_cost records a cost.
        let (pool, _dir) = setup_test_db().await;
        insert_catalog_model(&pool, "catalog-model", 1.5, 6.0).await;

        let store = ConfigStore::load(pool).await.unwrap();
        let price = store.pricing().await;
        let looked = price.lookup("catalog-model", None);
        assert_eq!(
            (looked.prompt, looked.completion),
            (1.5, 6.0),
            "layer 4 (model_registry) must populate the accounting carrier"
        );
    }

    #[tokio::test]
    async fn pricing_layer3_yaml_beats_catalog() {
        let (pool, _dir) = setup_test_db().await;
        insert_catalog_model(&pool, "shared-model", 1.5, 6.0).await;

        let store = ConfigStore::load(pool).await.unwrap();
        // set_bootstrap_pricing installs layer 3 *after* load(), mimicking
        // bootstrap.rs ordering.
        let mut yaml = crate::config::pricing::PricingConfig::default();
        yaml.models.insert(
            "shared-model".to_string(),
            crate::config::pricing::ModelPricing {
                default: crate::config::pricing::PriceEntry {
                    prompt: 9.0,
                    completion: 90.0,
                },
                tenants: std::collections::HashMap::new(),
            },
        );
        store.set_bootstrap_pricing(yaml).await;

        let price = store.pricing().await;
        let looked = price.lookup("shared-model", None);
        assert_eq!(
            (looked.prompt, looked.completion),
            (9.0, 90.0),
            "layer 3 (YAML) must override layer 4 (catalog)"
        );
    }

    #[tokio::test]
    async fn pricing_layer1_and_2_db_override_beats_yaml() {
        let (pool, _dir) = setup_test_db().await;
        insert_catalog_model(&pool, "m", 1.0, 2.0).await;

        let store = ConfigStore::load(pool.clone()).await.unwrap();
        store.set_bootstrap_pricing(pricing_yaml_with_model()).await;

        // Layer 2: global override (tenant_id NULL).
        sqlx::query(
            "INSERT INTO pricing_override \
             (model_id, tenant_id, input_price_per_1m, output_price_per_1m, effective_from) \
             VALUES ('m', NULL, 3.0, 4.0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Layer 1: tenant-specific override.
        sqlx::query(
            "INSERT INTO pricing_override \
             (model_id, tenant_id, input_price_per_1m, output_price_per_1m, effective_from) \
             VALUES ('m', 'vip', 7.0, 8.0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        store.refresh_from_db().await.unwrap();
        let price = store.pricing().await;

        let vip = price.lookup("m", Some("vip"));
        assert_eq!(
            (vip.prompt, vip.completion),
            (7.0, 8.0),
            "layer 1 (tenant override) must win"
        );

        let global = price.lookup("m", None);
        assert_eq!(
            (global.prompt, global.completion),
            (3.0, 4.0),
            "layer 2 (global override) must beat YAML and catalog"
        );

        let other = price.lookup("m", Some("other"));
        assert_eq!(
            (other.prompt, other.completion),
            (3.0, 4.0),
            "an unlisted tenant falls back to the global override"
        );
    }

    #[tokio::test]
    async fn pricing_override_outside_validity_window_is_ignored() {
        let (pool, _dir) = setup_test_db().await;
        insert_catalog_model(&pool, "m", 1.0, 2.0).await;

        let store = ConfigStore::load(pool.clone()).await.unwrap();

        let now = now_ms();
        // Expired one hour ago.
        sqlx::query(
            "INSERT INTO pricing_override \
             (model_id, tenant_id, input_price_per_1m, output_price_per_1m, \
              effective_from, effective_until) \
             VALUES ('m', NULL, 50.0, 60.0, ?1, ?2)",
        )
        .bind(now - 7_200_000)
        .bind(now - 3_600_000)
        .execute(&pool)
        .await
        .unwrap();
        // Not yet in force (starts one hour from now).
        sqlx::query(
            "INSERT INTO pricing_override \
             (model_id, tenant_id, input_price_per_1m, output_price_per_1m, effective_from) \
             VALUES ('m', NULL, 70.0, 80.0, ?1)",
        )
        .bind(now + 3_600_000)
        .execute(&pool)
        .await
        .unwrap();

        store.refresh_from_db().await.unwrap();
        let price = store.pricing().await;
        let looked = price.lookup("m", None);
        assert_eq!(
            (looked.prompt, looked.completion),
            (1.0, 2.0),
            "expired and future overrides must not apply; layer 4 stays"
        );
    }

    #[tokio::test]
    async fn pricing_hot_reload_picks_up_new_override() {
        // Editing pricing_override in the DB must change the effective price
        // after refresh_from_db(), with no restart.
        let (pool, _dir) = setup_test_db().await;
        insert_catalog_model(&pool, "m", 1.0, 2.0).await;
        let store = ConfigStore::load(pool.clone()).await.unwrap();

        let before = store.pricing().await.lookup("m", None).clone();
        assert_eq!((before.prompt, before.completion), (1.0, 2.0));

        sqlx::query(
            "INSERT INTO pricing_override \
             (model_id, tenant_id, input_price_per_1m, output_price_per_1m, effective_from) \
             VALUES ('m', NULL, 11.0, 12.0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        store.refresh_from_db().await.unwrap();

        let after = store.pricing().await.lookup("m", None).clone();
        assert_eq!(
            (after.prompt, after.completion),
            (11.0, 12.0),
            "a new override must be visible after refresh_from_db"
        );
    }

    #[tokio::test]
    async fn pricing_refresh_preserves_yaml_base_layer() {
        // Guard against the ordering hazard in bootstrap.rs: set_bootstrap_pricing
        // runs after load(), so a later refresh must not wipe the YAML layer.
        let (pool, _dir) = setup_test_db().await;
        insert_catalog_model(&pool, "other-model", 1.0, 1.0).await;
        let store = ConfigStore::load(pool.clone()).await.unwrap();

        let mut yaml = crate::config::pricing::PricingConfig::default();
        yaml.models.insert(
            "other-model".to_string(),
            crate::config::pricing::ModelPricing {
                default: crate::config::pricing::PriceEntry {
                    prompt: 42.0,
                    completion: 43.0,
                },
                tenants: std::collections::HashMap::new(),
            },
        );
        store.set_bootstrap_pricing(yaml).await;
        store.refresh_from_db().await.unwrap();

        let price = store.pricing().await;
        let looked = price.lookup("other-model", None);
        assert_eq!(
            (looked.prompt, looked.completion),
            (42.0, 43.0),
            "refresh_from_db must not drop the YAML base layer"
        );
    }

    #[tokio::test]
    async fn unpriced_model_still_looks_up_zero() {
        // A model absent from every layer keeps the pre-existing zero-price
        // behaviour, so compute_cost records NULL rather than inventing a cost.
        let (pool, _dir) = setup_test_db().await;
        let store = ConfigStore::load(pool).await.unwrap();
        let looked = store.pricing().await.lookup("never-seen", None).clone();
        assert_eq!((looked.prompt, looked.completion), (0.0, 0.0));
    }

    // ── T177: Hot-reload regression tests ───────────────────────────────

    #[tokio::test]
    async fn t177_scenario_1_change_pool_weight() {
        let (pool, _dir) = setup_test_db().await;
        let store = Arc::new(ConfigStore::load(pool.clone()).await.unwrap());
        store.import_yaml(test_yaml_document()).await.unwrap();

        let initial_pools = store.snapshot().await.pool_configs;
        let initial_weight = initial_pools["test_pool"].keys[0].weight;
        assert_eq!(initial_weight, 1);

        let kh = crate::db::compute_key_hash("sk-test-abc");
        sqlx::query("UPDATE key_entry SET weight = 5 WHERE key_hash = ?1")
            .bind(&kh)
            .execute(&pool)
            .await
            .unwrap();

        store.refresh_from_db().await.unwrap();
        let updated_pools = store.snapshot().await.pool_configs;
        let updated_weight = updated_pools["test_pool"].keys[0].weight;
        assert_eq!(updated_weight, 5, "pool key weight should be hot-reloaded");
    }

    #[tokio::test]
    async fn t177_scenario_2_disable_provider() {
        let (pool, _dir) = setup_test_db().await;
        let store = Arc::new(ConfigStore::load(pool.clone()).await.unwrap());
        store.import_yaml(test_yaml_document()).await.unwrap();

        let providers = store.snapshot().await.providers;
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "test_provider");

        sqlx::query("UPDATE provider_config SET enabled = 0 WHERE id = 'test_provider'")
            .execute(&pool)
            .await
            .unwrap();

        store.refresh_from_db().await.unwrap();
        let providers = store.snapshot().await.providers;
        assert_eq!(
            providers.len(),
            0,
            "disabled provider should be excluded from active config"
        );
    }

    #[tokio::test]
    async fn t177_scenario_3_change_model_routing() {
        let (pool, _dir) = setup_test_db().await;
        let store = Arc::new(ConfigStore::load(pool.clone()).await.unwrap());
        store.import_yaml(test_yaml_document()).await.unwrap();

        sqlx::query("INSERT INTO key_pool (id, strategy) VALUES ('new_pool', 'weighted_random')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO key_entry (pool_id, key_hash, key_plain, weight) VALUES ('new_pool', 'hash-xyz', 'sk-new', 2)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "UPDATE routing_config SET pool_id = 'new_pool' WHERE logical_model = 'test-model'",
        )
        .execute(&pool)
        .await
        .unwrap();

        store.refresh_from_db().await.unwrap();
        let routing = store.snapshot().await.model_routing;
        assert_eq!(
            routing["test-model"].pool_id(),
            "new_pool",
            "model routing should be hot-reloaded to new pool"
        );
    }

    #[tokio::test]
    async fn import_yaml_preserves_upstream_model_alias() {
        let (pool, _dir) = setup_test_db().await;
        let store = ConfigStore::load(pool.clone()).await.unwrap();
        let yaml = "server:\n  host: 127.0.0.1\n  port: 4000\nauth:\n  client_keys: []\ndb:\n  path: ./test.db\nfailover:\n  enabled: true\npools:\n  test_pool:\n    keys:\n      - key: sk-test-abc\n        weight: 1\nproviders:\n  - id: test_provider\n    pool_id: test_pool\n    base_url: https://api.test.com/v1\nmodel_to_pool:\n  deepseek-v4-flash:\n    pool: test_pool\n    upstream_model: deepseek-v4-flash-0731\n";
        store.import_yaml(yaml).await.unwrap();
        let routing = store.snapshot().await.model_routing;
        assert_eq!(
            routing["deepseek-v4-flash"].upstream_model(),
            Some("deepseek-v4-flash-0731")
        );
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT upstream_model FROM routing_config WHERE logical_model = 'deepseek-v4-flash'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored.as_deref(), Some("deepseek-v4-flash-0731"));
    }

    #[tokio::test]
    async fn t177_poller_detects_changes() {
        let (pool, _dir) = setup_test_db().await;
        let store = Arc::new(ConfigStore::load(pool.clone()).await.unwrap());
        store.import_yaml(test_yaml_document()).await.unwrap();

        let v1 = store.version().await;

        sqlx::query(
            "INSERT INTO provider_config (id, kind, base_url, pool_id, enabled) VALUES ('poll_provider', 'gemini', 'https://api.gemini.com/v1', 'test_pool', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        store.refresh_from_db().await.unwrap();
        let v2 = store.version().await;
        assert!(v2 > v1, "version should increase after detecting changes");

        let providers = store.snapshot().await.providers;
        let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        assert!(
            ids.contains(&"poll_provider"),
            "new provider should appear after refresh"
        );
    }
}

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
    AlertConfig, FailoverConfig, KeyEntry, ModelMetadataConfig, ModelRouting, PoolConfig,
    PoolStrategy, ProviderConfig, ProviderKind,
};
use crate::error::AppError;

// ── In-memory config snapshots ──

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
    /// Pricing is a bootstrap/static carrier until a DB pricing table exists.
    /// It is shared by request accounting and dashboard cost views.
    pub pricing: Arc<crate::config::pricing::PricingConfig>,
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

    pub async fn set_bootstrap_pricing(&self, pricing: crate::config::pricing::PricingConfig) {
        self.inner.write().await.pricing = Arc::new(pricing);
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
        for (pool_id, pool_cfg) in &config.pools {
            sqlx::query("INSERT INTO key_pool (id, strategy, enabled) VALUES (?1, ?2, 1) ON CONFLICT(id) DO UPDATE SET strategy = excluded.strategy, enabled = 1")
                .bind(pool_id).bind(match pool_cfg.strategy { PoolStrategy::WeightedRandom => "weighted_random" })
                .execute(&mut *tx).await.map_err(|e| AppError::Internal(format!("ConfigStore import pool: {e}")))?;
            sqlx::query("DELETE FROM key_entry WHERE pool_id = ?1")
                .bind(pool_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::Internal(format!("ConfigStore import pool keys: {e}")))?;
            for key_entry in &pool_cfg.keys {
                let kh = crate::db::compute_key_hash(&key_entry.key);
                sqlx::query("INSERT INTO key_entry (pool_id, key_hash, key_plain, weight, enabled) VALUES (?1, ?2, ?3, ?4, 1)")
                    .bind(pool_id).bind(kh).bind(&key_entry.key).bind(key_entry.weight as i64)
                    .execute(&mut *tx).await.map_err(|e| AppError::Internal(format!("ConfigStore import key: {e}")))?;
            }
        }
        for provider in &config.providers {
            let mut metadata = provider.metadata.clone();
            if !metadata.is_object() {
                metadata = serde_json::json!({});
            }
            if let Some(object) = metadata.as_object_mut() {
                if let Some(api_version) = &provider.api_version {
                    object.insert(
                        "api_version".into(),
                        serde_json::Value::String(api_version.clone()),
                    );
                }
                if let Some(region) = &provider.region {
                    object.insert("region".into(), serde_json::Value::String(region.clone()));
                }
            }
            sqlx::query("INSERT INTO provider_config (id, kind, base_url, pool_id, enabled, metadata) VALUES (?1, ?2, ?3, ?4, 1, ?5) ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, base_url = excluded.base_url, pool_id = excluded.pool_id, enabled = 1, metadata = excluded.metadata")
                .bind(&provider.id).bind(provider_kind_to_str(&provider.kind)).bind(&provider.base_url)
                .bind(&provider.pool_id).bind(serde_json::to_string(&metadata).unwrap_or_default())
                .execute(&mut *tx).await.map_err(|e| AppError::Internal(format!("ConfigStore import provider: {e}")))?;
        }
        for (model, routing) in &config.model_to_pool {
            let params = routing
                .default_params()
                .map(|v| serde_json::to_string(v).unwrap_or_default());
            sqlx::query("DELETE FROM routing_config WHERE logical_model = ?1")
                .bind(model)
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::Internal(format!("ConfigStore import routing: {e}")))?;
            sqlx::query("INSERT INTO routing_config (logical_model, pool_id, default_params, enabled) VALUES (?1, ?2, ?3, 1)")
                .bind(model).bind(routing.pool_id()).bind(params.as_deref()).execute(&mut *tx).await
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

        let mut current = self.inner.write().await;
        current.providers = Arc::new(providers);
        current.pool_configs = Arc::new(pool_configs);
        current.model_routing = Arc::new(model_routing);
        current.model_registry = Arc::new(model_registry);
        current.version += 1;

        tracing::debug!(version = current.version, "ConfigStore: cache refreshed");
        Ok(())
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
                let api_version = metadata
                    .get("api_version")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                let region = metadata
                    .get("region")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                Ok(ProviderConfig {
                    id: r.id,
                    kind: str_to_provider_kind(&r.kind)?,
                    base_url: r.base_url,
                    pool_id: r.pool_id,
                    api_version,
                    region,
                    metadata,
                })
            })
            .collect()
    }

    async fn load_pools_from_db(&self) -> Result<HashMap<String, PoolConfig>, AppError> {
        #[derive(sqlx::FromRow)]
        struct DbKeyEntry {
            pool_id: String,
            key_plain: String,
            weight: i64,
        }

        // Load pools
        #[derive(sqlx::FromRow)]
        struct DbPool {
            id: String,
            strategy: String,
        }

        let db_pools: Vec<DbPool> =
            sqlx::query_as("SELECT id, strategy FROM key_pool WHERE enabled = 1")
                .fetch_all(&self.db)
                .await
                .map_err(|e| AppError::Internal(format!("ConfigStore load pools: {e}")))?;

        // Load keys
        let db_keys: Vec<DbKeyEntry> =
            sqlx::query_as("SELECT pool_id, key_plain, weight FROM key_entry WHERE enabled = 1")
                .fetch_all(&self.db)
                .await
                .map_err(|e| AppError::Internal(format!("ConfigStore load keys: {e}")))?;

        // Group keys by pool_id
        let mut keys_by_pool: HashMap<String, Vec<KeyEntry>> = HashMap::new();
        for k in db_keys {
            keys_by_pool.entry(k.pool_id).or_default().push(KeyEntry {
                key: k.key_plain,
                weight: k.weight as u32,
            });
        }

        let mut pools = HashMap::new();
        for p in db_pools {
            let keys = keys_by_pool.remove(&p.id).unwrap_or_default();
            pools.insert(
                p.id.clone(),
                PoolConfig {
                    keys,
                    strategy: match p.strategy.as_str() {
                        "weighted_random" => PoolStrategy::WeightedRandom,
                        _ => PoolStrategy::WeightedRandom,
                    },
                },
            );
        }

        Ok(pools)
    }

    async fn load_routing_from_db(&self) -> Result<HashMap<String, ModelRouting>, AppError> {
        #[derive(sqlx::FromRow)]
        struct DbRouting {
            logical_model: String,
            pool_id: String,
            default_params: Option<String>,
        }

        let rows: Vec<DbRouting> = sqlx::query_as(
            "SELECT logical_model, pool_id, default_params FROM routing_config WHERE enabled = 1",
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
                let routing = if default_params.is_object() {
                    ModelRouting::WithParams {
                        pool: r.pool_id,
                        default_params,
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
        ProviderKind::Azure => "azure",
        ProviderKind::Bedrock => "bedrock",
        ProviderKind::Cohere => "cohere",
        ProviderKind::Mistral => "mistral",
        ProviderKind::Ollama => "ollama",
        ProviderKind::Vllm => "vllm",
    }
}

fn str_to_provider_kind(s: &str) -> Result<ProviderKind, AppError> {
    match s {
        "openai" => Ok(ProviderKind::OpenAi),
        "anthropic" => Ok(ProviderKind::Anthropic),
        "gemini" => Ok(ProviderKind::Gemini),
        "azure" => Ok(ProviderKind::Azure),
        "bedrock" => Ok(ProviderKind::Bedrock),
        "cohere" => Ok(ProviderKind::Cohere),
        "mistral" => Ok(ProviderKind::Mistral),
        "ollama" => Ok(ProviderKind::Ollama),
        "vllm" => Ok(ProviderKind::Vllm),
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
    async fn import_and_reload_preserve_provider_specific_parameters() {
        let (pool, _dir) = setup_test_db().await;
        let store = ConfigStore::load(pool.clone()).await.unwrap();
        let yaml = "server:\n  host: 127.0.0.1\n  port: 4000\nauth:\n  client_keys: []\ndb:\n  path: ./test.db\nfailover:\n  enabled: true\npools:\n  shared:\n    keys:\n      - key: sk-provider-test\n        weight: 1\nproviders:\n  - id: azure-east\n    kind: azure\n    pool_id: shared\n    base_url: https://azure.example/v1\n    api_version: 2024-02-01\n  - id: bedrock-west\n    kind: bedrock\n    pool_id: shared\n    base_url: https://bedrock.example\n    region: us-west-2\nmodel_to_pool:\n  test-model: shared\n";
        store.import_yaml(yaml).await.unwrap();
        let snapshot = store.snapshot().await;
        let azure = snapshot
            .providers
            .iter()
            .find(|p| p.id == "azure-east")
            .unwrap();
        let bedrock = snapshot
            .providers
            .iter()
            .find(|p| p.id == "bedrock-west")
            .unwrap();
        assert_eq!(azure.api_version.as_deref(), Some("2024-02-01"));
        assert_eq!(bedrock.region.as_deref(), Some("us-west-2"));

        let reloaded = ConfigStore::load(pool).await.unwrap();
        let snapshot = reloaded.snapshot().await;
        let azure = snapshot
            .providers
            .iter()
            .find(|p| p.id == "azure-east")
            .unwrap();
        let bedrock = snapshot
            .providers
            .iter()
            .find(|p| p.id == "bedrock-west")
            .unwrap();
        assert_eq!(azure.api_version.as_deref(), Some("2024-02-01"));
        assert_eq!(bedrock.region.as_deref(), Some("us-west-2"));
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

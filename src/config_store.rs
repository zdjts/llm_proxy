//! DB-backed configuration store with hot-reload (v4.0 Track H — T174).
//!
//! Replaces direct reads of `config.providers`, `config.pools`, and
//! `config.model_to_pool` from YAML with a DB-backed, polling-refreshed
//! in-memory cache. See `docs/adr-016-config-as-data.md`.
//!
//! # Lifecycle
//!
//! 1. `ConfigStore::load()` — syncs the Managed sections (`pools`, `providers`,
//!    `model_to_pool`) from YAML into the DB, then loads the DB into the cache.
//!    `config.yaml` is authoritative at boot: editing it and restarting applies
//!    the change.
//! 2. `spawn_config_poller()` — background task polls DB every N seconds,
//!    atomically swapping the in-memory cache when changes are detected.
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
    KeyEntry, ModelRouting, PoolConfig, PoolStrategy, ProviderConfig, ProviderKind,
};
use crate::error::AppError;

// ── In-memory config snapshots ──

/// Full snapshot of all managed configuration sections.
/// Every field is behind `Arc` so readers get a consistent view without
/// holding a lock across `.await`.
#[derive(Debug, Clone)]
pub struct ManagedConfig {
    pub providers: Arc<Vec<ProviderConfig>>,
    pub pool_configs: Arc<HashMap<String, PoolConfig>>,
    pub model_routing: Arc<HashMap<String, ModelRouting>>,
    /// v4.0 AUDIT-14 Fix: model_registry loaded from DB (capabilities + pricing).
    pub model_registry: Arc<Vec<serde_json::Value>>,
    /// v4.0 AUDIT-14 Fix: shadow_route loaded from DB (not yet wired into router).
    pub shadow_routes: Arc<Vec<serde_json::Value>>,
    pub version: u64,
}

impl Default for ManagedConfig {
    fn default() -> Self {
        Self {
            providers: Arc::new(Vec::new()),
            pool_configs: Arc::new(HashMap::new()),
            model_routing: Arc::new(HashMap::new()),
            model_registry: Arc::new(Vec::new()),
            shadow_routes: Arc::new(Vec::new()),
            version: 0,
        }
    }
}

// ── ConfigStore ──

/// Central store for DB-managed configuration.
///
/// All public methods return clones of `Arc`-wrapped data, so callers
/// never hold the lock across I/O.
pub struct ConfigStore {
    inner: RwLock<ManagedConfig>,
    db: SqlitePool,
}

impl ConfigStore {
    /// Create a new ConfigStore and load initial data from the DB.
    ///
    /// `config.yaml` is authoritative at boot: the Managed sections
    /// (`pools`, `providers`, `model_to_pool`) are synced into the DB on every
    /// startup, then the in-memory cache is loaded from the DB.
    pub async fn load(
        db: SqlitePool,
        yaml_config: &crate::config::Config,
    ) -> Result<Self, AppError> {
        let store = Self {
            inner: RwLock::new(ManagedConfig::default()),
            db,
        };

        store.sync_from_yaml(yaml_config).await?;
        store.refresh_from_db().await?;

        Ok(store)
    }

    /// Sync the Managed sections from `config.yaml` into the DB.
    ///
    /// Runs on every startup so editing YAML and restarting takes effect.
    /// Entities the YAML defines are upserted (YAML wins); entities added later
    /// through the Admin API and absent from YAML are left untouched.
    async fn sync_from_yaml(&self, yaml_config: &crate::config::Config) -> Result<(), AppError> {
        // Pools → key_pool + key_entry (keys are replaced wholesale per pool)
        for (pool_id, pool_cfg) in &yaml_config.pools {
            sqlx::query(
                "INSERT INTO key_pool (id, strategy, enabled) VALUES (?1, ?2, 1) \
                 ON CONFLICT(id) DO UPDATE SET strategy = excluded.strategy, enabled = 1",
            )
            .bind(pool_id)
            .bind(match pool_cfg.strategy {
                PoolStrategy::WeightedRandom => "weighted_random",
            })
            .execute(&self.db)
            .await
            .map_err(|e| AppError::Internal(format!("ConfigStore sync pool: {e}")))?;

            sqlx::query("DELETE FROM key_entry WHERE pool_id = ?1")
                .bind(pool_id)
                .execute(&self.db)
                .await
                .map_err(|e| AppError::Internal(format!("ConfigStore sync pool keys: {e}")))?;

            for key_entry in &pool_cfg.keys {
                let kh = crate::db::compute_key_hash(&key_entry.key);
                sqlx::query(
                    "INSERT INTO key_entry (pool_id, key_hash, key_plain, weight, enabled) \
                     VALUES (?1, ?2, ?3, ?4, 1)",
                )
                .bind(pool_id)
                .bind(&kh)
                .bind(&key_entry.key)
                .bind(key_entry.weight as i64)
                .execute(&self.db)
                .await
                .map_err(|e| AppError::Internal(format!("ConfigStore sync key: {e}")))?;
            }
        }

        // Providers → provider_config (upsert; weight / bad_status_codes_override preserved)
        for provider in &yaml_config.providers {
            let kind_str = provider_kind_to_str(&provider.kind);
            sqlx::query(
                "INSERT INTO provider_config (id, kind, base_url, pool_id, enabled, metadata) \
                 VALUES (?1, ?2, ?3, ?4, 1, ?5) \
                 ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, \
                 base_url = excluded.base_url, pool_id = excluded.pool_id, \
                 enabled = 1, metadata = excluded.metadata",
            )
            .bind(&provider.id)
            .bind(kind_str)
            .bind(&provider.base_url)
            .bind(&provider.pool_id)
            .bind(serde_json::to_string(&provider.metadata).unwrap_or_default())
            .execute(&self.db)
            .await
            .map_err(|e| AppError::Internal(format!("ConfigStore sync provider: {e}")))?;
        }

        // model_to_pool → routing_config (replace per logical model)
        for (model, routing) in &yaml_config.model_to_pool {
            let pool_id = routing.pool_id();
            let default_params = routing
                .default_params()
                .map(|v| serde_json::to_string(v).unwrap_or_default());
            sqlx::query("DELETE FROM routing_config WHERE logical_model = ?1")
                .bind(model)
                .execute(&self.db)
                .await
                .map_err(|e| AppError::Internal(format!("ConfigStore sync routing: {e}")))?;
            sqlx::query(
                "INSERT INTO routing_config (logical_model, pool_id, default_params, enabled) \
                 VALUES (?1, ?2, ?3, 1)",
            )
            .bind(model)
            .bind(pool_id)
            .bind(default_params.as_deref())
            .execute(&self.db)
            .await
            .map_err(|e| AppError::Internal(format!("ConfigStore sync routing: {e}")))?;
        }

        tracing::info!(
            pools = yaml_config.pools.len(),
            providers = yaml_config.providers.len(),
            models = yaml_config.model_to_pool.len(),
            "ConfigStore: synced managed config from config.yaml into DB"
        );

        Ok(())
    }

    /// Refresh the in-memory cache from DB tables.
    ///
    /// Reads all managed tables and atomically swaps the cache.
    pub async fn refresh_from_db(&self) -> Result<(), AppError> {
        let providers = self.load_providers_from_db().await?;
        let pool_configs = self.load_pools_from_db().await?;
        let model_routing = self.load_routing_from_db().await?;
        let model_registry = self.load_model_registry_from_db().await?;
        let shadow_routes = self.load_shadow_routes_from_db().await?;

        let mut current = self.inner.write().await;
        current.providers = Arc::new(providers);
        current.pool_configs = Arc::new(pool_configs);
        current.model_routing = Arc::new(model_routing);
        current.model_registry = Arc::new(model_registry);
        current.shadow_routes = Arc::new(shadow_routes);
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

        Ok(rows
            .into_iter()
            .map(|r| ProviderConfig {
                id: r.id,
                kind: str_to_provider_kind(&r.kind),
                base_url: r.base_url,
                pool_id: r.pool_id,
                api_version: None, // will be extracted from metadata JSON
                region: None,      // will be extracted from metadata JSON
                metadata: r
                    .metadata
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
            })
            .collect())
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
    async fn load_model_registry_from_db(&self) -> Result<Vec<serde_json::Value>, AppError> {
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
        }

        let rows: Vec<ModelRow> = sqlx::query_as(
            "SELECT id, display_name, provider_kind, provider_config_id, \
             supports_vision, supports_tool_calling, supports_json_mode, \
             max_context_tokens, max_output_tokens \
             FROM model_registry WHERE enabled = 1",
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("ConfigStore load model_registry: {e}")))?;

        let result: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "display_name": r.display_name,
                    "provider_kind": r.provider_kind,
                    "provider_config_id": r.provider_config_id,
                    "supports_vision": r.supports_vision != 0,
                    "supports_tool_calling": r.supports_tool_calling != 0,
                    "supports_json_mode": r.supports_json_mode != 0,
                    "max_context_tokens": r.max_context_tokens,
                    "max_output_tokens": r.max_output_tokens,
                })
            })
            .collect();
        Ok(result)
    }

    /// AUDIT-14 Fix: Load shadow_route table from DB.
    ///
    /// NOTE: shadow_route data is loaded into ConfigStore cache but not yet
    /// wired into the Router's request path. Shadow routing integration is
    /// tracked for a future Track. See AUDIT-14.
    async fn load_shadow_routes_from_db(&self) -> Result<Vec<serde_json::Value>, AppError> {
        #[derive(sqlx::FromRow)]
        struct ShadowRow {
            id: i32,
            logical_model: String,
            primary_physical: String,
            shadow_physical: String,
            shadow_ratio: f64,
        }

        let rows: Vec<ShadowRow> = sqlx::query_as(
            "SELECT id, logical_model, primary_physical, shadow_physical, \
             shadow_ratio FROM shadow_route WHERE enabled = 1",
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("ConfigStore load shadow_route: {e}")))?;

        let result: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "logical_model": r.logical_model,
                    "primary_physical": r.primary_physical,
                    "shadow_physical": r.shadow_physical,
                    "shadow_ratio": r.shadow_ratio,
                })
            })
            .collect();
        Ok(result)
    }

    // ── Public accessors (return Arc clones, no lock held across .await) ──

    /// Get current provider configs.
    pub async fn get_providers(&self) -> Arc<Vec<ProviderConfig>> {
        Arc::clone(&self.inner.read().await.providers)
    }

    /// Get current pool configs.
    pub async fn get_pools(&self) -> Arc<HashMap<String, PoolConfig>> {
        Arc::clone(&self.inner.read().await.pool_configs)
    }

    /// Get current model routing.
    pub async fn get_model_routing(&self) -> Arc<HashMap<String, ModelRouting>> {
        Arc::clone(&self.inner.read().await.model_routing)
    }

    /// Get current model registry entries (AUDIT-14 Fix).
    pub async fn get_model_registry(&self) -> Arc<Vec<serde_json::Value>> {
        Arc::clone(&self.inner.read().await.model_registry)
    }

    /// Get current shadow routes (AUDIT-14 Fix — not yet wired into Router).
    pub async fn get_shadow_routes(&self) -> Arc<Vec<serde_json::Value>> {
        Arc::clone(&self.inner.read().await.shadow_routes)
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

fn str_to_provider_kind(s: &str) -> ProviderKind {
    match s {
        "openai" => ProviderKind::OpenAi,
        "anthropic" => ProviderKind::Anthropic,
        "gemini" => ProviderKind::Gemini,
        "azure" => ProviderKind::Azure,
        "bedrock" => ProviderKind::Bedrock,
        "cohere" => ProviderKind::Cohere,
        "mistral" => ProviderKind::Mistral,
        "ollama" => ProviderKind::Ollama,
        "vllm" => ProviderKind::Vllm,
        _ => {
            tracing::warn!(kind = %s, "ConfigStore: unknown provider kind, defaulting to openai");
            ProviderKind::OpenAi
        }
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

    fn minimal_yaml_config() -> crate::config::Config {
        crate::config::Config {
            server: crate::config::ServerConfig {
                host: "127.0.0.1".into(),
                port: 4000,
                max_body_bytes: 10485760,
            },
            auth: crate::config::AuthConfig {
                client_keys: vec![],
            },
            db: crate::config::DbConfig {
                path: "./test.db".into(),
            },
            failover: crate::config::FailoverConfig {
                enabled: true,
                bad_status_codes: vec![401, 402, 403, 429],
                max_retries: 1,
                probe_interval_secs: 60,
                probe_timeout_secs: 10,
                max_probe_retries: 3,
            },
            pools: {
                let mut m = HashMap::new();
                m.insert(
                    "test_pool".into(),
                    PoolConfig {
                        keys: vec![KeyEntry {
                            key: "sk-test-abc".into(),
                            weight: 1,
                        }],
                        strategy: PoolStrategy::WeightedRandom,
                    },
                );
                m
            },
            providers: vec![ProviderConfig {
                id: "test_provider".into(),
                pool_id: "test_pool".into(),
                base_url: "https://api.test.com/v1".into(),
                kind: ProviderKind::OpenAi,
                api_version: None,
                region: None,
                metadata: serde_json::Value::Null,
            }],
            model_to_pool: {
                let mut m = HashMap::new();
                m.insert(
                    "test-model".into(),
                    ModelRouting::Simple("test_pool".into()),
                );
                m
            },
            bootstrap_admin: Default::default(),
            admin: Default::default(),
            pricing: Default::default(),
            rate_limit: Default::default(),
            cache_max_entries: 256,
            alerts: Default::default(),
            acl: Default::default(),
            fallback_models: Default::default(),
            concurrency: Default::default(),
        }
    }

    #[tokio::test]
    async fn it_imports_once_from_yaml() {
        let (pool, _dir) = setup_test_db().await;
        let yaml_cfg = minimal_yaml_config();

        let store = ConfigStore::load(pool, &yaml_cfg).await.unwrap();

        let providers = store.get_providers().await;
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "test_provider");

        let pools = store.get_pools().await;
        assert!(pools.contains_key("test_pool"));
        assert_eq!(pools["test_pool"].keys.len(), 1);

        let routing = store.get_model_routing().await;
        assert_eq!(routing["test-model"].pool_id(), "test_pool");

        assert!(store.version().await > 0);
    }

    #[tokio::test]
    async fn it_does_not_double_import() {
        let (pool, _dir) = setup_test_db().await;
        let yaml_cfg = minimal_yaml_config();

        // First load: sync from YAML
        let store = ConfigStore::load(pool.clone(), &yaml_cfg).await.unwrap();
        assert_eq!(store.get_providers().await.len(), 1);

        // Second load: re-sync is idempotent — no duplicated rows
        let store2 = ConfigStore::load(pool, &yaml_cfg).await.unwrap();
        assert_eq!(store2.get_providers().await.len(), 1);
        assert_eq!(store2.get_pools().await["test_pool"].keys.len(), 1);
        assert_eq!(store2.get_model_routing().await.len(), 1);
    }

    #[tokio::test]
    async fn it_overwrites_db_with_yaml_on_reload() {
        let (pool, _dir) = setup_test_db().await;
        let yaml_cfg = minimal_yaml_config();
        let _store = ConfigStore::load(pool.clone(), &yaml_cfg).await.unwrap();

        // Simulate an Admin-API edit that conflicts with YAML (weight 1 → 9)
        let kh = crate::db::compute_key_hash("sk-test-abc");
        sqlx::query("UPDATE key_entry SET weight = 9 WHERE key_hash = ?1")
            .bind(&kh)
            .execute(&pool)
            .await
            .unwrap();

        // Reload: config.yaml is authoritative at boot, so weight reverts to YAML (1)
        let store2 = ConfigStore::load(pool, &yaml_cfg).await.unwrap();
        let pools = store2.get_pools().await;
        assert_eq!(
            pools["test_pool"].keys[0].weight, 1,
            "YAML should win over DB edits on restart"
        );
    }

    #[tokio::test]
    async fn it_applies_edited_yaml_on_reload() {
        let (pool, _dir) = setup_test_db().await;
        let yaml_cfg = minimal_yaml_config();
        let _store = ConfigStore::load(pool.clone(), &yaml_cfg).await.unwrap();

        // User edits config.yaml: new pool + re-route the model
        let mut yaml2 = minimal_yaml_config();
        yaml2.pools.insert(
            "new_pool".into(),
            PoolConfig {
                keys: vec![KeyEntry {
                    key: "sk-new-abc".into(),
                    weight: 2,
                }],
                strategy: PoolStrategy::WeightedRandom,
            },
        );
        yaml2
            .model_to_pool
            .insert("test-model".into(), ModelRouting::Simple("new_pool".into()));

        let store2 = ConfigStore::load(pool, &yaml2).await.unwrap();

        let routing = store2.get_model_routing().await;
        assert_eq!(routing["test-model"].pool_id(), "new_pool");
        assert!(store2.get_pools().await.contains_key("new_pool"));
        assert_eq!(store2.get_pools().await["new_pool"].keys.len(), 1);
    }

    #[tokio::test]
    async fn it_refreshes_from_db_after_write() {
        let (pool, _dir) = setup_test_db().await;
        let yaml_cfg = minimal_yaml_config();
        let store = Arc::new(ConfigStore::load(pool.clone(), &yaml_cfg).await.unwrap());

        // Insert a new provider directly into DB
        sqlx::query(
            "INSERT INTO provider_config (id, kind, base_url, pool_id, enabled) \
             VALUES ('new_provider', 'anthropic', 'https://api.anthropic.com/v1', 'test_pool', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Refresh
        store.refresh_from_db().await.unwrap();
        let providers = store.get_providers().await;
        assert_eq!(providers.len(), 2);
    }

    #[tokio::test]
    async fn it_handles_empty_db_without_yaml_data() {
        let (pool, _dir) = setup_test_db().await;
        let mut yaml_cfg = minimal_yaml_config();
        yaml_cfg.pools.clear();
        yaml_cfg.providers.clear();
        yaml_cfg.model_to_pool.clear();

        let store = ConfigStore::load(pool, &yaml_cfg).await.unwrap();
        assert_eq!(store.get_providers().await.len(), 0);
        assert_eq!(store.get_pools().await.len(), 0);
    }

    #[tokio::test]
    async fn it_handles_refresh_with_no_changes() {
        let (pool, _dir) = setup_test_db().await;
        let yaml_cfg = minimal_yaml_config();
        let store = ConfigStore::load(pool, &yaml_cfg).await.unwrap();
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
        let yaml_cfg = minimal_yaml_config();
        let store = Arc::new(ConfigStore::load(pool.clone(), &yaml_cfg).await.unwrap());

        let initial_pools = store.get_pools().await;
        let initial_weight = initial_pools["test_pool"].keys[0].weight;
        assert_eq!(initial_weight, 1);

        let kh = crate::db::compute_key_hash("sk-test-abc");
        sqlx::query("UPDATE key_entry SET weight = 5 WHERE key_hash = ?1")
            .bind(&kh)
            .execute(&pool)
            .await
            .unwrap();

        store.refresh_from_db().await.unwrap();
        let updated_pools = store.get_pools().await;
        let updated_weight = updated_pools["test_pool"].keys[0].weight;
        assert_eq!(updated_weight, 5, "pool key weight should be hot-reloaded");
    }

    #[tokio::test]
    async fn t177_scenario_2_disable_provider() {
        let (pool, _dir) = setup_test_db().await;
        let yaml_cfg = minimal_yaml_config();
        let store = Arc::new(ConfigStore::load(pool.clone(), &yaml_cfg).await.unwrap());

        let providers = store.get_providers().await;
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "test_provider");

        sqlx::query("UPDATE provider_config SET enabled = 0 WHERE id = 'test_provider'")
            .execute(&pool)
            .await
            .unwrap();

        store.refresh_from_db().await.unwrap();
        let providers = store.get_providers().await;
        assert_eq!(
            providers.len(),
            0,
            "disabled provider should be excluded from active config"
        );
    }

    #[tokio::test]
    async fn t177_scenario_3_change_model_routing() {
        let (pool, _dir) = setup_test_db().await;
        let yaml_cfg = minimal_yaml_config();
        let store = Arc::new(ConfigStore::load(pool.clone(), &yaml_cfg).await.unwrap());

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
        let routing = store.get_model_routing().await;
        assert_eq!(
            routing["test-model"].pool_id(),
            "new_pool",
            "model routing should be hot-reloaded to new pool"
        );
    }

    #[tokio::test]
    async fn t177_poller_detects_changes() {
        let (pool, _dir) = setup_test_db().await;
        let yaml_cfg = minimal_yaml_config();
        let store = Arc::new(ConfigStore::load(pool.clone(), &yaml_cfg).await.unwrap());

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

        let providers = store.get_providers().await;
        let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        assert!(
            ids.contains(&"poll_provider"),
            "new provider should appear after refresh"
        );
    }
}

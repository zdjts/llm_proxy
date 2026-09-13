//! Application assembly: YAML load → stores → AppState (ADR-017 / ADR-018).
//!
//! `main` only parses env, calls [`bootstrap`], binds, and serves.
//! RuntimePolicy / rate-limit / cache-max / alerts are **startup-static**.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::{broadcast, watch};

use crate::aggregator;
use crate::alerts::{self, AlertSnapshot};
use crate::auth;
use crate::auth_store::AuthStore;
use crate::cache::PromptCache;
use crate::circuit_breaker::CircuitBreaker;
use crate::concurrency::ConcurrencyLimiter;
use crate::config::ProviderKind;
use crate::config::{Config, FailoverConfig};
use crate::config_store::ConfigStore;
use crate::credential::CredentialRuntime;
use crate::db;
use crate::db_maintenance::{self, DbMaintenanceConfig};
use crate::health;
use crate::metrics::Metrics;
use crate::provider::anthropic::AnthropicProvider;
use crate::provider::gemini::GeminiProvider;
use crate::provider::openai::OpenAiProvider;
use crate::provider::registry::ProviderRegistry;
use crate::ratelimit::RateLimiter;
use crate::router::{BadKeyRegistry, RouterHandle};
use crate::runtime;
use crate::server::{self, AppState};

/// Shared handles needed to serve HTTP without constructing stores in `main`.
pub struct BootedApp {
    pub app: axum::Router,
    pub addr: SocketAddr,
    pub shutdown_tx: watch::Sender<()>,
    pub health_handle: tokio::task::JoinHandle<()>,
}

/// Load YAML, open DB, seed ConfigStore, spawn background tasks, build axum app.
/// Does not bind a TCP port.
pub async fn bootstrap(config_path: &str) -> anyhow::Result<BootedApp> {
    let config = match Config::load(std::path::Path::new(config_path)) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            anyhow::bail!("Config error: {e}");
        }
    };

    let pool = db::connect(&config.db.path).await?;

    let bad_keys = Arc::new(BadKeyRegistry::new());

    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(OpenAiFactory));
    registry.register(Arc::new(AnthropicFactory));
    registry.register(Arc::new(GeminiFactory));
    registry.register(Arc::new(AzureFactory));
    registry.register(Arc::new(BedrockFactory));
    registry.register(Arc::new(CohereFactory));
    registry.register(Arc::new(MistralFactory));
    registry.register(Arc::new(OllamaFactory));
    registry.register(Arc::new(VllmFactory));

    let bad_status_codes: Arc<[u16]> =
        Arc::from(config.failover.bad_status_codes.clone().into_boxed_slice());

    // ConfigStore is the runtime source for pools/providers/routing (ADR-017).
    let config_store = Arc::new(ConfigStore::load(pool.clone()).await?);

    let routing_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM routing_config WHERE enabled = 1")
            .fetch_one(&pool)
            .await?;
    // Default: seed managed tables only when empty (ADR-017). Set
    // LLM_PROXY_SYNC_YAML=1 to force a full replace from the startup YAML file
    // (useful when copying config.yaml between hosts).
    let force_yaml_sync = std::env::var("LLM_PROXY_SYNC_YAML")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false);
    if force_yaml_sync || routing_count == 0 {
        let yaml = tokio::fs::read_to_string(config_path).await?;
        config_store.import_yaml(&yaml).await?;
        if force_yaml_sync {
            tracing::info!("synchronized managed configuration from YAML (LLM_PROXY_SYNC_YAML)");
        } else {
            tracing::info!("initialized managed configuration from YAML");
        }
    }
    config_store
        .set_bootstrap_pricing(config.pricing.clone())
        .await;
    config_store
        .set_bootstrap_model_metadata(config.model_metadata.clone())
        .await;
    // Failover / alerts / cache-max: YAML-static RuntimePolicy, not hot-reloaded.
    config_store
        .set_bootstrap_runtime(crate::config_store::RuntimePolicy {
            failover: config.failover.clone(),
            alerts: config.alerts.clone(),
            cache_max_entries: config.cache_max_entries,
        })
        .await;

    let missing_registry_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM routing_config r LEFT JOIN model_registry m ON m.id = r.logical_model AND m.enabled = 1 WHERE r.enabled = 1 AND m.id IS NULL",
    )
    .fetch_one(&pool)
    .await?;
    if missing_registry_count > 0 {
        match crate::model_import::import_default(&pool, false).await {
            Ok(report) => {
                tracing::info!(
                    new = report.counts.new,
                    skipped = report.counts.skipped,
                    conflicts = report.counts.conflicts,
                    "initialized model registry from models-store.json"
                );
                for reason in report.reasons {
                    tracing::warn!(reason = %reason, "model metadata import skipped an entry");
                }
                config_store.refresh_from_db().await?;
            }
            Err(error) => {
                tracing::warn!(%error, "model metadata import failed; continuing with configured metadata");
            }
        }
    }
    let registry = Arc::new(registry);

    let router_handle = RouterHandle::new(
        runtime::rebuild_router(
            &config_store,
            &registry,
            Arc::clone(&bad_status_codes),
            Arc::clone(&bad_keys),
        )
        .await?,
    );

    let pools_for_health: HashMap<String, crate::config::PoolConfig> =
        (*config_store.snapshot().await.pool_configs).clone();
    let health_providers = registry
        .build_all(
            &config_store.snapshot().await.providers,
            Arc::clone(&bad_status_codes),
        )
        .await?;

    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let health_config = FailoverConfig {
        enabled: true,
        bad_status_codes: config.failover.bad_status_codes.clone(),
        max_retries: config.failover.max_retries,
        probe_interval_secs: config.failover.probe_interval_secs,
        probe_timeout_secs: config.failover.probe_timeout_secs,
        max_probe_retries: config.failover.max_probe_retries,
    };

    let credentials = Arc::new(CredentialRuntime::new(Some(pool.clone())));

    let health_handle = health::spawn_health_task(
        Arc::clone(&bad_keys),
        pools_for_health,
        health_providers,
        health_config,
        Arc::clone(&credentials),
        shutdown_rx,
    );

    let _aggregator_handle = aggregator::spawn_aggregator(pool.clone(), shutdown_tx.subscribe());

    let circuit_breaker = Arc::new(CircuitBreaker::with_defaults());
    let concurrency = Arc::new(ConcurrencyLimiter::new(
        config.concurrency.max_per_tenant,
        config.concurrency.total_max,
    ));
    let fallback_config = Arc::new(config.fallback_models.clone());

    let _db_maint_handle = db_maintenance::spawn_db_maintenance(
        pool.clone(),
        DbMaintenanceConfig::default(),
        shutdown_tx.subscribe(),
    );

    let _config_poller = crate::config_store::spawn_config_poller(
        Arc::clone(&config_store),
        5,
        shutdown_tx.subscribe(),
    );

    let _router_rebuilder = spawn_router_rebuilder(
        Arc::clone(&config_store),
        Arc::clone(&registry),
        Arc::clone(&bad_status_codes),
        Arc::clone(&bad_keys),
        router_handle.clone(),
        shutdown_tx.subscribe(),
    );

    let (alert_tx, _) = broadcast::channel::<crate::alerts::AlertEvent>(512);
    let alert_snapshot: AlertSnapshot = Arc::new(Mutex::new(VecDeque::new()));

    // Rate limit is YAML-only (ADR-017); no ConfigStore carrier.
    let rate_limiter = if config.rate_limit.enabled {
        Some(Arc::new(RateLimiter::new(
            config.rate_limit.requests_per_minute,
            alert_tx.clone(),
        )))
    } else {
        None
    };

    let _alert_handle = if config.alerts.enabled {
        let mut channels: Vec<Arc<dyn crate::alerts::channel::AlertChannel>> = Vec::new();

        let wh_url = if config.alerts.channels.webhook.url.is_empty() {
            config.alerts.webhook_url.clone()
        } else {
            config.alerts.channels.webhook.url.clone()
        };
        if config.alerts.channels.webhook.enabled || !wh_url.is_empty() {
            channels.push(Arc::new(crate::alerts::channel::WebhookChannel {
                url: wh_url,
                secret: config.alerts.webhook_secret.clone(),
            }));
        }

        if config.alerts.channels.slack.enabled {
            channels.push(Arc::new(crate::alerts::channel::SlackChannel {
                url: config.alerts.channels.slack.url.clone(),
            }));
        }

        if config.alerts.channels.discord.enabled {
            channels.push(Arc::new(crate::alerts::channel::DiscordChannel {
                url: config.alerts.channels.discord.url.clone(),
            }));
        }

        if config.alerts.channels.email.enabled {
            channels.push(Arc::new(crate::alerts::channel::EmailChannel {
                to: config.alerts.channels.email.to.clone(),
            }));
        }

        let shutdown_rx = shutdown_tx.subscribe();
        let alert_rx = alert_tx.subscribe();
        Some(alerts::spawn_alert_task(
            pool.clone(),
            channels,
            shutdown_rx,
            alert_rx,
            alert_snapshot.clone(),
        ))
    } else {
        None
    };

    let metrics = Arc::new(Metrics::default());

    let auth_store = Arc::new(AuthStore::new(config.auth.client_keys.clone()));

    let app_state = AppState {
        router: router_handle.clone(),
        catalog: crate::model_catalog::ModelCatalog::new(
            router_handle.clone(),
            Arc::clone(&config_store),
        ),
        db: pool.clone(),
        config: Arc::clone(&config),
        config_store: Arc::clone(&config_store),
        credentials: Arc::clone(&credentials),
        cache: PromptCache::new(config.cache_max_entries),
        metrics: Arc::clone(&metrics),
        circuit_breaker: Arc::clone(&circuit_breaker),
        concurrency: Arc::clone(&concurrency),
        fallback_config: Arc::clone(&fallback_config),
        alert_tx: alert_tx.clone(),
        error_burst_counters: Arc::new(dashmap::DashMap::new()),
        alert_snapshot: alert_snapshot.clone(),
        auth_store: Some(Arc::clone(&auth_store)),
    };

    let auth_state = auth::AuthState {
        store: Some(Arc::clone(&auth_store)),
        entries: config.auth.client_keys.clone(),
    };

    let app = server::build_router(app_state, auth_state, rate_limiter);

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid bind address: {e}"))?;

    Ok(BootedApp {
        app,
        addr,
        shutdown_tx,
        health_handle,
    })
}

type RoutingSnapshot = runtime::RoutingSnapshot;

fn spawn_router_rebuilder(
    store: Arc<ConfigStore>,
    registry: Arc<ProviderRegistry>,
    bad_status_codes: Arc<[u16]>,
    bad_keys: Arc<BadKeyRegistry>,
    handle: RouterHandle,
    mut shutdown: tokio::sync::watch::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.tick().await;

        let mut last: Option<RoutingSnapshot> = Some((
            store.snapshot().await.providers,
            store.snapshot().await.pool_configs,
            store.snapshot().await.model_routing,
        ));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let providers = store.snapshot().await.providers;
                    let pools = store.snapshot().await.pool_configs;
                    let routing = store.snapshot().await.model_routing;

                    let changed = match &last {
                        None => true,
                        Some((lp, lpo, lr)) => lp != &providers || lpo != &pools || lr != &routing,
                    };

                    if changed {
                        match runtime::rebuild_router(
                            &store,
                            &registry,
                            Arc::clone(&bad_status_codes),
                            Arc::clone(&bad_keys),
                        )
                        .await
                        {
                            Ok(router) => {
                                last = Some((providers, pools, routing));
                                handle.swap(router);
                                tracing::info!("router rebuilt from config store");
                            }
                            Err(e) => {
                                tracing::error!(
                                    error = %e,
                                    "router rebuild failed — keeping previous router"
                                );
                            }
                        }
                    }
                }
                _ = shutdown.changed() => {
                    tracing::info!("router rebuilder shutting down");
                    return;
                }
            }
        }
    })
}

struct OpenAiFactory;

#[async_trait::async_trait]
impl crate::provider::registry::ProviderFactory for OpenAiFactory {
    fn kind_name(&self) -> &str {
        "openai"
    }

    async fn create(
        &self,
        config: &crate::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn crate::provider::Provider>, crate::error::AppError> {
        Ok(Arc::new(OpenAiProvider::new(
            config.id.clone(),
            config.base_url.clone(),
            bad_status_codes,
        )))
    }

    fn supports(&self, kind: &ProviderKind) -> bool {
        matches!(kind, ProviderKind::OpenAi)
    }
}

struct AnthropicFactory;

#[async_trait::async_trait]
impl crate::provider::registry::ProviderFactory for AnthropicFactory {
    fn kind_name(&self) -> &str {
        "anthropic"
    }

    async fn create(
        &self,
        config: &crate::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn crate::provider::Provider>, crate::error::AppError> {
        Ok(Arc::new(AnthropicProvider::new(
            config.id.clone(),
            config.base_url.clone(),
            bad_status_codes,
        )))
    }

    fn supports(&self, kind: &ProviderKind) -> bool {
        matches!(kind, ProviderKind::Anthropic)
    }
}

struct GeminiFactory;

#[async_trait::async_trait]
impl crate::provider::registry::ProviderFactory for GeminiFactory {
    fn kind_name(&self) -> &str {
        "gemini"
    }

    async fn create(
        &self,
        config: &crate::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn crate::provider::Provider>, crate::error::AppError> {
        Ok(Arc::new(GeminiProvider::new(
            config.id.clone(),
            config.base_url.clone(),
            bad_status_codes,
        )))
    }

    fn supports(&self, kind: &ProviderKind) -> bool {
        matches!(kind, ProviderKind::Gemini)
    }
}

struct AzureFactory;

#[async_trait::async_trait]
impl crate::provider::registry::ProviderFactory for AzureFactory {
    fn kind_name(&self) -> &str {
        "azure"
    }

    async fn create(
        &self,
        config: &crate::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn crate::provider::Provider>, crate::error::AppError> {
        use crate::provider::azure::AzureProvider;
        let api_version = config
            .api_version
            .clone()
            .unwrap_or_else(|| "2024-06-01".to_string());
        Ok(Arc::new(AzureProvider::new(
            config.id.clone(),
            config.base_url.clone(),
            api_version,
            bad_status_codes,
        )))
    }

    fn supports(&self, kind: &ProviderKind) -> bool {
        matches!(kind, ProviderKind::Azure)
    }
}

struct BedrockFactory;

#[async_trait::async_trait]
impl crate::provider::registry::ProviderFactory for BedrockFactory {
    fn kind_name(&self) -> &str {
        "bedrock"
    }

    async fn create(
        &self,
        config: &crate::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn crate::provider::Provider>, crate::error::AppError> {
        use crate::provider::bedrock::BedrockProvider;
        let region = config
            .region
            .clone()
            .unwrap_or_else(|| "us-east-1".to_string());
        Ok(Arc::new(BedrockProvider::new(
            config.id.clone(),
            config.base_url.clone(),
            region,
            bad_status_codes,
        )))
    }

    fn supports(&self, kind: &ProviderKind) -> bool {
        matches!(kind, ProviderKind::Bedrock)
    }
}

struct CohereFactory;

#[async_trait::async_trait]
impl crate::provider::registry::ProviderFactory for CohereFactory {
    fn kind_name(&self) -> &str {
        "cohere"
    }

    async fn create(
        &self,
        config: &crate::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn crate::provider::Provider>, crate::error::AppError> {
        use crate::provider::cohere::CohereProvider;
        Ok(Arc::new(CohereProvider::new(
            config.id.clone(),
            config.base_url.clone(),
            bad_status_codes,
        )))
    }

    fn supports(&self, kind: &ProviderKind) -> bool {
        matches!(kind, ProviderKind::Cohere)
    }
}

struct MistralFactory;

#[async_trait::async_trait]
impl crate::provider::registry::ProviderFactory for MistralFactory {
    fn kind_name(&self) -> &str {
        "mistral"
    }

    async fn create(
        &self,
        config: &crate::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn crate::provider::Provider>, crate::error::AppError> {
        use crate::provider::mistral::MistralProvider;
        Ok(Arc::new(MistralProvider::new(
            config.id.clone(),
            config.base_url.clone(),
            bad_status_codes,
        )))
    }

    fn supports(&self, kind: &ProviderKind) -> bool {
        matches!(kind, ProviderKind::Mistral)
    }
}

struct OllamaFactory;

#[async_trait::async_trait]
impl crate::provider::registry::ProviderFactory for OllamaFactory {
    fn kind_name(&self) -> &str {
        "ollama"
    }

    async fn create(
        &self,
        config: &crate::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn crate::provider::Provider>, crate::error::AppError> {
        use crate::provider::ollama::OllamaProvider;
        Ok(Arc::new(OllamaProvider::new(
            config.id.clone(),
            config.base_url.clone(),
            bad_status_codes,
        )))
    }

    fn supports(&self, kind: &ProviderKind) -> bool {
        matches!(kind, ProviderKind::Ollama)
    }
}

struct VllmFactory;

#[async_trait::async_trait]
impl crate::provider::registry::ProviderFactory for VllmFactory {
    fn kind_name(&self) -> &str {
        "vllm"
    }

    async fn create(
        &self,
        config: &crate::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn crate::provider::Provider>, crate::error::AppError> {
        use crate::provider::vllm::VllmProvider;
        Ok(Arc::new(VllmProvider::new(
            config.id.clone(),
            config.base_url.clone(),
            bad_status_codes,
        )))
    }

    fn supports(&self, kind: &ProviderKind) -> bool {
        matches!(kind, ProviderKind::Vllm)
    }
}

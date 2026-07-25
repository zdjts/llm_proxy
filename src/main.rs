//! Binary entrypoint for `llm_proxy` (v2.0).
//!
//! Startup sequence:
//! 1. Initialise tracing subscriber
//! 2. Load and validate config
//! 3. Open SQLite pool + run migrations
//! 4. Build provider registry + instantiate providers
//! 5. Build router with pool registrations
//! 6. Initialize AuthStore (dynamic key management)
//! 7. Initialize QuotaTracker (per-tenant quotas)
//! 8. Initialize Pipeline (request/response transforms)
//! 9. Spawn background health-probe task
//! 10. Start axum HTTP server with graceful shutdown

use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::signal;
use tokio::sync::{broadcast, watch};

use llm_proxy::aggregator;
use llm_proxy::alerts::{self, AlertSnapshot};
use llm_proxy::auth;
use llm_proxy::auth_store::AuthStore;
use llm_proxy::cache::PromptCache;
use llm_proxy::circuit_breaker::CircuitBreaker;
use llm_proxy::concurrency::ConcurrencyLimiter;
use llm_proxy::config::ProviderKind;
use llm_proxy::config::{Config, FailoverConfig, ModelRouting, PoolConfig, ProviderConfig};
use llm_proxy::config_store::ConfigStore;
use llm_proxy::db;
use llm_proxy::db_maintenance::{self, DbMaintenanceConfig};
use llm_proxy::health;
use llm_proxy::metrics::Metrics;
use llm_proxy::pipeline::Pipeline;
use llm_proxy::provider::anthropic::AnthropicProvider;
use llm_proxy::provider::gemini::GeminiProvider;
use llm_proxy::provider::openai::OpenAiProvider;
use llm_proxy::provider::registry::ProviderRegistry;
use llm_proxy::quota::{QuotaConfig, QuotaTracker};
use llm_proxy::ratelimit::RateLimiter;
use llm_proxy::router::{BadKeyRegistry, Router, RouterHandle};
use llm_proxy::server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config_path =
        std::env::var("LLM_PROXY_CONFIG").unwrap_or_else(|_| "config.yaml".to_string());

    let config = match Config::load(std::path::Path::new(&config_path)) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("Config error: {e}");
            std::process::exit(1);
        }
    };

    let pool = db::connect(&config.db.path).await?;

    let bad_keys = Arc::new(BadKeyRegistry::new());

    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(OpenAiFactory));
    registry.register(Arc::new(AnthropicFactory));
    registry.register(Arc::new(GeminiFactory));
    // ── v3.0: Register 6 new provider factories (Fix 4) ──
    registry.register(Arc::new(AzureFactory));
    registry.register(Arc::new(BedrockFactory));
    registry.register(Arc::new(CohereFactory));
    registry.register(Arc::new(MistralFactory));
    registry.register(Arc::new(OllamaFactory));
    registry.register(Arc::new(VllmFactory));

    let bad_status_codes: Arc<[u16]> =
        Arc::from(config.failover.bad_status_codes.clone().into_boxed_slice());

    // ── v3.0: Seed built-in RBAC roles (Fix 1) ──
    if let Err(e) = llm_proxy::rbac::store::seed_builtin_roles(&pool).await {
        tracing::warn!(error = %e, "Failed to seed built-in RBAC roles — continuing");
    }
    llm_proxy::rbac::store::bootstrap_admin(&pool, &config.bootstrap_admin).await?;

    // ── v4.0 Track H (T174-T175): ConfigStore replaces YAML managed config ──
    let config_store = Arc::new(ConfigStore::load(pool.clone(), &config).await?);
    let registry = Arc::new(registry);

    let router_handle = RouterHandle::new(
        rebuild_router(
            &config_store,
            &registry,
            Arc::clone(&bad_status_codes),
            Arc::clone(&bad_keys),
        )
        .await?,
    );

    // Health task snapshots pools/providers at startup; new pools added later
    // via ConfigStore hot-reload are still serviced through the live router.
    let pools_for_health: HashMap<String, llm_proxy::config::PoolConfig> =
        (*config_store.get_pools().await).clone();
    let health_providers = registry
        .build_all(
            &config_store.get_providers().await,
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

    let health_handle = health::spawn_health_task(
        Arc::clone(&bad_keys),
        pools_for_health,
        health_providers,
        health_config,
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

    // ── v4.0 Track H: ConfigStore hot-reload poller ──
    let _config_poller = llm_proxy::config_store::spawn_config_poller(
        Arc::clone(&config_store),
        5, // poll every 5 seconds
        shutdown_tx.subscribe(),
    );

    // Rebuild the live router when the ConfigStore detects a change, so pool /
    // routing / provider edits take effect without a restart.
    let _router_rebuilder = spawn_router_rebuilder(
        Arc::clone(&config_store),
        Arc::clone(&registry),
        Arc::clone(&bad_status_codes),
        Arc::clone(&bad_keys),
        router_handle.clone(),
        shutdown_tx.subscribe(),
    );

    let (alert_tx, _) = broadcast::channel::<llm_proxy::alerts::AlertEvent>(512);
    let alert_snapshot: AlertSnapshot = Arc::new(Mutex::new(VecDeque::new()));

    let rate_limiter = if config.rate_limit.enabled {
        Some(Arc::new(RateLimiter::new(
            config.rate_limit.requests_per_minute,
            alert_tx.clone(),
        )))
    } else {
        None
    };

    let _alert_handle = if config.alerts.enabled {
        let mut channels: Vec<Arc<dyn llm_proxy::alerts::channel::AlertChannel>> = Vec::new();

        let wh_url = if config.alerts.channels.webhook.url.is_empty() {
            config.alerts.webhook_url.clone()
        } else {
            config.alerts.channels.webhook.url.clone()
        };
        if config.alerts.channels.webhook.enabled || !wh_url.is_empty() {
            channels.push(Arc::new(llm_proxy::alerts::channel::WebhookChannel {
                url: wh_url,
                secret: config.alerts.webhook_secret.clone(),
            }));
        }

        if config.alerts.channels.slack.enabled {
            channels.push(Arc::new(llm_proxy::alerts::channel::SlackChannel {
                url: config.alerts.channels.slack.url.clone(),
            }));
        }

        if config.alerts.channels.discord.enabled {
            channels.push(Arc::new(llm_proxy::alerts::channel::DiscordChannel {
                url: config.alerts.channels.discord.url.clone(),
            }));
        }

        if config.alerts.channels.email.enabled {
            channels.push(Arc::new(llm_proxy::alerts::channel::EmailChannel {
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

    let quota_tracker = QuotaConfig {
        enabled: false,
        daily_tokens: None,
        monthly_requests: None,
    };
    let quota_tracker = Some(Arc::new(QuotaTracker::new(quota_tracker, alert_tx.clone())));

    let pipeline = Some(Arc::new(Pipeline::from_config(
        &llm_proxy::pipeline::PipelineConfig::default(),
    )));

    // ── v3.0 RBAC (Fix 1): JWT service + compat mode ──
    let jwt_secret = std::env::var("LLM_PROXY_JWT_SECRET")
        .unwrap_or_else(|_| "llm-proxy-default-jwt-secret-change-me".to_string());
    let jwt_service = Arc::new(llm_proxy::rbac::session::JwtService::new(
        jwt_secret.as_bytes(),
    ));
    let rbac_state = llm_proxy::rbac::middleware::RbacState {
        pool: pool.clone(),
        jwt: jwt_service,
        // compat_mode: when no Bearer token is present, grant full Owner access.
        // This ensures existing IP-whitelist deployments keep working without
        // any config change. Set LLM_PROXY_RBAC_NO_COMPAT=1 to disable.
        compat_mode: std::env::var("LLM_PROXY_RBAC_NO_COMPAT").is_err(),
    };
    if rbac_state.compat_mode {
        tracing::warn!(
            "RBAC running in compat mode — all unauthenticated requests get full Owner access. \
             Set LLM_PROXY_RBAC_NO_COMPAT=1 to enforce JWT authentication."
        );
    }

    let app_state = server::AppState {
        router: router_handle,
        db: pool.clone(),
        config: Arc::clone(&config),
        config_store: Some(config_store),
        budget_manager: Some(Arc::new(llm_proxy::budget::BudgetManager::new(
            pool.clone(),
        ))),
        cache: PromptCache::new(config.cache_max_entries),
        metrics: Arc::clone(&metrics),
        circuit_breaker: Arc::clone(&circuit_breaker),
        concurrency: Arc::clone(&concurrency),
        fallback_config: Arc::clone(&fallback_config),
        alert_tx: alert_tx.clone(),
        error_burst_counters: Arc::new(dashmap::DashMap::new()),
        alert_snapshot: alert_snapshot.clone(),
        auth_store: Some(auth_store),
        quota_tracker,
        pipeline,
        rbac_state: Some(rbac_state),
    };

    let auth_state = auth::AuthState {
        entries: config.auth.client_keys.clone(),
    };

    let app = server::build_router(app_state, auth_state, rate_limiter);

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .expect("invalid bind address");

    tracing::info!("listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = signal::ctrl_c().await;
            tracing::info!("shutting down...");
            let _ = shutdown_tx.send(());
        })
        .await?;

    let _ = tokio::time::timeout(std::time::Duration::from_secs(30), health_handle).await;

    tracing::info!("llm_proxy stopped");
    Ok(())
}

/// Build a fresh [`Router`] from the current ConfigStore snapshot.
///
/// Used both for the initial boot and by [`spawn_router_rebuilder`] when the
/// DB-backed config changes. Returns an error (leaving the previous router
/// active) if a model references a pool that no longer exists.
async fn rebuild_router(
    store: &ConfigStore,
    registry: &ProviderRegistry,
    bad_status_codes: Arc<[u16]>,
    bad_keys: Arc<BadKeyRegistry>,
) -> Result<Arc<Router>, llm_proxy::error::AppError> {
    let providers = registry
        .build_all(&store.get_providers().await, bad_status_codes)
        .await?;
    let pools = store.get_pools().await;
    let routing = store.get_model_routing().await;

    for (model, r) in routing.iter() {
        if !pools.contains_key(r.pool_id()) {
            return Err(llm_proxy::error::AppError::Config(format!(
                "pool '{}' referenced by model '{model}' not found in configured pools",
                r.pool_id()
            )));
        }
    }

    let mut model_map: HashMap<String, (String, PoolConfig, Option<serde_json::Value>)> =
        HashMap::new();
    for (model, r) in routing.iter() {
        let pool_id = r.pool_id().to_string();
        let pool = pools.get(&pool_id).cloned().ok_or_else(|| {
            llm_proxy::error::AppError::Config(format!("pool '{pool_id}' not found"))
        })?;
        model_map.insert(model.clone(), (pool_id, pool, r.default_params().cloned()));
    }

    Ok(Arc::new(Router::new(model_map, providers, bad_keys)))
}

/// Snapshot of the config sections that drive routing decisions.
type RoutingSnapshot = (
    Arc<Vec<ProviderConfig>>,
    Arc<HashMap<String, PoolConfig>>,
    Arc<HashMap<String, ModelRouting>>,
);

/// Poll the ConfigStore for changes and swap in a rebuilt router.
///
/// Rebuilds are skipped when the providers / pools / routing snapshot is
/// unchanged, so a no-op ConfigStore refresh (which still bumps its version
/// counter) does not churn provider instances.
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
            store.get_providers().await,
            store.get_pools().await,
            store.get_model_routing().await,
        ));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let providers = store.get_providers().await;
                    let pools = store.get_pools().await;
                    let routing = store.get_model_routing().await;

                    let changed = match &last {
                        None => true,
                        Some((lp, lpo, lr)) => lp != &providers || lpo != &pools || lr != &routing,
                    };

                    if changed {
                        last = Some((providers, pools, routing));
                        match rebuild_router(
                            &store,
                            &registry,
                            Arc::clone(&bad_status_codes),
                            Arc::clone(&bad_keys),
                        )
                        .await
                        {
                            Ok(router) => {
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
impl llm_proxy::provider::registry::ProviderFactory for OpenAiFactory {
    fn kind_name(&self) -> &str {
        "openai"
    }

    async fn create(
        &self,
        config: &llm_proxy::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn llm_proxy::provider::Provider>, llm_proxy::error::AppError> {
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
impl llm_proxy::provider::registry::ProviderFactory for AnthropicFactory {
    fn kind_name(&self) -> &str {
        "anthropic"
    }

    async fn create(
        &self,
        config: &llm_proxy::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn llm_proxy::provider::Provider>, llm_proxy::error::AppError> {
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
impl llm_proxy::provider::registry::ProviderFactory for GeminiFactory {
    fn kind_name(&self) -> &str {
        "gemini"
    }

    async fn create(
        &self,
        config: &llm_proxy::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn llm_proxy::provider::Provider>, llm_proxy::error::AppError> {
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

// ── v3.0: 6 new provider factories (Fix 4) ──

struct AzureFactory;

#[async_trait::async_trait]
impl llm_proxy::provider::registry::ProviderFactory for AzureFactory {
    fn kind_name(&self) -> &str {
        "azure"
    }

    async fn create(
        &self,
        config: &llm_proxy::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn llm_proxy::provider::Provider>, llm_proxy::error::AppError> {
        use llm_proxy::provider::azure::AzureProvider;
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
impl llm_proxy::provider::registry::ProviderFactory for BedrockFactory {
    fn kind_name(&self) -> &str {
        "bedrock"
    }

    async fn create(
        &self,
        config: &llm_proxy::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn llm_proxy::provider::Provider>, llm_proxy::error::AppError> {
        use llm_proxy::provider::bedrock::BedrockProvider;
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
impl llm_proxy::provider::registry::ProviderFactory for CohereFactory {
    fn kind_name(&self) -> &str {
        "cohere"
    }

    async fn create(
        &self,
        config: &llm_proxy::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn llm_proxy::provider::Provider>, llm_proxy::error::AppError> {
        use llm_proxy::provider::cohere::CohereProvider;
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
impl llm_proxy::provider::registry::ProviderFactory for MistralFactory {
    fn kind_name(&self) -> &str {
        "mistral"
    }

    async fn create(
        &self,
        config: &llm_proxy::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn llm_proxy::provider::Provider>, llm_proxy::error::AppError> {
        use llm_proxy::provider::mistral::MistralProvider;
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
impl llm_proxy::provider::registry::ProviderFactory for OllamaFactory {
    fn kind_name(&self) -> &str {
        "ollama"
    }

    async fn create(
        &self,
        config: &llm_proxy::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn llm_proxy::provider::Provider>, llm_proxy::error::AppError> {
        use llm_proxy::provider::ollama::OllamaProvider;
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
impl llm_proxy::provider::registry::ProviderFactory for VllmFactory {
    fn kind_name(&self) -> &str {
        "vllm"
    }

    async fn create(
        &self,
        config: &llm_proxy::config::ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn llm_proxy::provider::Provider>, llm_proxy::error::AppError> {
        use llm_proxy::provider::vllm::VllmProvider;
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

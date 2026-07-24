//! Binary entrypoint for `llm_proxy`.
//!
//! # Startup sequence
//!
//! 1. Initialise tracing subscriber
//! 2. Load and validate config (`config.yaml` or `LLM_PROXY_CONFIG`)
//! 3. Open SQLite pool + run migrations
//! 4. Build router with pool registrations
//! 5. Spawn background health-probe task
//! 6. Start axum HTTP server with graceful shutdown on SIGTERM/SIGINT

use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::signal;
use tokio::sync::{broadcast, watch};

use llm_proxy::aggregator;
use llm_proxy::alerts::{self, AlertSnapshot};
use llm_proxy::auth;
use llm_proxy::cache::PromptCache;
use llm_proxy::config::ProviderKind;
use llm_proxy::config::{Config, FailoverConfig};
use llm_proxy::db;
use llm_proxy::health;
use llm_proxy::provider::anthropic::AnthropicProvider;
use llm_proxy::provider::gemini::GeminiProvider;
use llm_proxy::provider::openai::OpenAiProvider;
use llm_proxy::ratelimit::RateLimiter;
use llm_proxy::router::{BadKeyRegistry, Router};
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

    let mut providers: HashMap<String, Arc<dyn llm_proxy::provider::Provider>> = HashMap::new();
    let bad_status_codes: Arc<[u16]> =
        Arc::from(config.failover.bad_status_codes.clone().into_boxed_slice());

    for p in &config.providers {
        let prov: Arc<dyn llm_proxy::provider::Provider> = match p.kind {
            ProviderKind::OpenAi => Arc::new(OpenAiProvider::new(
                p.id.clone(),
                p.base_url.clone(),
                Arc::clone(&bad_status_codes),
            )),
            ProviderKind::Anthropic => Arc::new(AnthropicProvider::new(
                p.id.clone(),
                p.base_url.clone(),
                Arc::clone(&bad_status_codes),
            )),
            ProviderKind::Gemini => Arc::new(GeminiProvider::new(
                p.id.clone(),
                p.base_url.clone(),
                Arc::clone(&bad_status_codes),
            )),
        };
        providers.insert(p.pool_id.clone(), prov);
    }

    let model_map: HashMap<
        String,
        (
            String,
            llm_proxy::config::PoolConfig,
            Option<serde_json::Value>,
        ),
    > = config
        .model_to_pool
        .iter()
        .map(|(model, routing)| {
            let pool_id = routing.pool_id().to_string();
            let pool_cfg = config.pools.get(&pool_id).cloned().unwrap_or_else(|| {
                panic!("pool '{pool_id}' not found for model '{model}'");
            });
            let params = routing.default_params().cloned();
            (model.clone(), (pool_id, pool_cfg, params))
        })
        .collect();

    let pools_clone: HashMap<String, llm_proxy::config::PoolConfig> = config.pools.clone();
    let router = Arc::new(Router::new(
        model_map,
        providers.clone(),
        Arc::clone(&bad_keys),
    ));

    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let health_config = FailoverConfig {
        enabled: true,
        bad_status_codes: config.failover.bad_status_codes.clone(),
        probe_interval_secs: config.failover.probe_interval_secs,
        probe_timeout_secs: config.failover.probe_timeout_secs,
        max_probe_retries: config.failover.max_probe_retries,
    };

    let health_handle = health::spawn_health_task(
        Arc::clone(&bad_keys),
        pools_clone,
        providers,
        health_config,
        shutdown_rx,
    );

    let _aggregator_handle = aggregator::spawn_aggregator(pool.clone(), shutdown_tx.subscribe());

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

        // Webhook: backward compat — fall back to deprecated top-level webhook_url
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

    let app_state = server::AppState {
        router: Arc::clone(&router),
        db: pool,
        config: Arc::clone(&config),
        cache: PromptCache::new(config.cache_max_entries),
        alert_tx: alert_tx.clone(),
        error_burst_counters: std::sync::Arc::new(dashmap::DashMap::new()),
        alert_snapshot: alert_snapshot.clone(),
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

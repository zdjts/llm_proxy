//! HTTP server layer: axum router, middleware chain, and request handlers (v2.0).
//!
//! Frontend served as static SPA from `frontend/dist/`. All Admin API routes
//! remain as REST + WebSocket endpoints. CORS enabled by default.

pub mod handler;
pub mod middleware;
pub mod stream_normalize;

use std::sync::Arc;

use axum::Router;
use dashmap::DashMap;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::alerts::{AlertEvent, AlertSnapshot};
use crate::auth;
use crate::cache::PromptCache;
use crate::circuit_breaker::CircuitBreaker;
use crate::concurrency::ConcurrencyLimiter;
use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::credential::CredentialRuntime;
use crate::metrics::Metrics;
use crate::model_catalog::ModelCatalog;
use crate::router::RouterHandle;

#[derive(Clone)]
pub struct AppState {
    pub router: RouterHandle,
    pub catalog: ModelCatalog,
    pub db: sqlx::SqlitePool,
    pub config: Arc<Config>,
    pub cache: PromptCache,
    pub metrics: Arc<Metrics>,
    pub circuit_breaker: Arc<CircuitBreaker>,
    pub concurrency: Arc<ConcurrencyLimiter>,
    pub alert_tx: tokio::sync::broadcast::Sender<AlertEvent>,
    pub error_burst_counters: Arc<DashMap<(String, String), u32>>,
    pub alert_snapshot: AlertSnapshot,
    pub auth_store: Option<Arc<crate::auth_store::AuthStore>>,
    /// v4.0 Track H: DB-backed config store for hot-reload and admin CRUD.
    pub config_store: Arc<ConfigStore>,
    /// OAuth access-token cache and refresh.
    pub credentials: Arc<CredentialRuntime>,
}

pub fn build_router(
    state: AppState,
    auth_state: auth::AuthState,
    rate_limiter: Option<Arc<crate::ratelimit::RateLimiter>>,
) -> Router {
    let limit = state.config.server.max_body_bytes;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api = Router::new()
        .route(
            "/v1/chat/completions",
            axum::routing::post(handler::chat_completions_handler),
        )
        .route("/v1/models", axum::routing::get(handler::models_handler))
        .route(
            "/v1/model-metadata",
            axum::routing::get(handler::model_metadata_handler),
        )
        .route(
            "/metrics",
            axum::routing::get(crate::metrics::metrics_handler),
        )
        .route(
            "/health",
            axum::routing::get(crate::health_check::health_full),
        )
        .route(
            "/health/live",
            axum::routing::get(crate::health_check::health_live),
        )
        .route(
            "/health/ready",
            axum::routing::get(crate::health_check::health_ready),
        );

    // Request pipeline (outer → inner): request_id → auth → body_size →
    // rate_limit → handler. Failover/retry lives in the handler/router,
    // never in Provider::chat. Layers are applied inner-first (tower onion).
    let api = if let Some(rl) = rate_limiter {
        api.route_layer(axum::middleware::from_fn_with_state(
            rl,
            crate::ratelimit::rate_limit_middleware,
        ))
    } else {
        api
    };

    // axum extractors (Json/Bytes) enforce DefaultBodyLimit (2 MiB) independently of
    // tower-http's RequestBodyLimitLayer. Raise both so max_body_bytes actually applies.
    let api = api
        .layer(axum::extract::DefaultBodyLimit::max(limit))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(limit))
        .route_layer(axum::middleware::from_fn_with_state(
            auth_state,
            auth::require_auth,
        ))
        .layer(axum::middleware::from_fn(middleware::request_id_middleware));

    let admin_allowed_ips = state.config.admin.allowed_ips.clone();

    let admin = build_admin_routes(state.clone(), admin_allowed_ips);

    Router::new()
        .merge(api.with_state(state.clone()))
        .nest("/admin", admin.with_state(state.clone()))
        .fallback_service(ServeDir::new("frontend/dist"))
        .layer(cors)
        .with_state(state)
}

fn build_admin_routes(state: AppState, allowed_ips: Vec<String>) -> Router<AppState> {
    let admin = Router::new()
        .route(
            "/",
            axum::routing::get(crate::dashboard::cost::cost_overview_handler),
        )
        .route(
            "/requests",
            axum::routing::get(crate::dashboard::requests::request_list_handler),
        )
        .route(
            "/requests/{id}",
            axum::routing::get(crate::dashboard::requests::request_detail_handler),
        )
        .route(
            "/keys",
            axum::routing::get(crate::dashboard::keys::key_health_handler),
        )
        .route(
            "/traffic",
            axum::routing::get(crate::dashboard::traffic::traffic_trend_handler),
        )
        .route(
            "/alerts",
            axum::routing::get(crate::dashboard::alerts::alerts_handler),
        )
        .route(
            "/cost/drilldown",
            axum::routing::get(crate::dashboard::cost_drilldown::cost_drilldown_handler),
        )
        .route(
            "/api/status",
            axum::routing::get(crate::dashboard::admin_api::admin_api_status),
        )
        .route(
            "/api/keys",
            axum::routing::get(crate::dashboard::admin_api::admin_api_keys),
        )
        .route(
            "/api/usage",
            axum::routing::get(crate::dashboard::usage::admin_api_usage),
        )
        .route(
            "/api/config",
            axum::routing::get(crate::dashboard::admin_api::admin_api_config_overview),
        )
        .route(
            "/api/config/export",
            axum::routing::get(crate::dashboard::admin_api::admin_api_config_export),
        )
        .route(
            "/api/config/validate",
            axum::routing::post(crate::dashboard::admin_api::admin_api_config_validate),
        )
        .route(
            "/api/config/import",
            axum::routing::post(crate::dashboard::admin_api::admin_api_config_import),
        )
        .route(
            "/api/config/refresh",
            axum::routing::post(crate::dashboard::admin_api::admin_api_config_refresh),
        )
        .route(
            "/api/models",
            axum::routing::get(crate::dashboard::admin_api::admin_api_models)
                .post(crate::dashboard::admin_api::admin_api_create_model),
        )
        .route(
            "/api/models/{model_id}",
            axum::routing::patch(crate::dashboard::admin_api::admin_api_update_model),
        )
        .route(
            "/api/client-keys",
            axum::routing::get(crate::dashboard::admin_api::admin_api_list_client_keys)
                .post(crate::dashboard::admin_api::admin_api_add_client_key),
        )
        .route(
            "/api/client-keys/{key_hash}",
            axum::routing::patch(crate::dashboard::admin_api::admin_api_update_client_key)
                .delete(crate::dashboard::admin_api::admin_api_delete_client_key)
                .post(crate::dashboard::admin_api::admin_api_rotate_client_key),
        )
        .route(
            "/live",
            axum::routing::get(crate::dashboard::live::live_handler),
        )
        .route(
            "/export",
            axum::routing::get(crate::dashboard::export::export_handler),
        )
        // ── v4.0 Track H: Config-as-Data CRUD (T176) ──
        .route(
            "/api/providers",
            axum::routing::get(crate::dashboard::admin_api::admin_api_list_providers)
                .post(crate::dashboard::admin_api::admin_api_create_provider),
        )
        .route(
            "/api/providers/{provider_id}",
            axum::routing::delete(crate::dashboard::admin_api::admin_api_delete_provider),
        )
        .route(
            "/api/pools",
            axum::routing::get(crate::dashboard::admin_api::admin_api_list_pools)
                .post(crate::dashboard::admin_api::admin_api_create_pool),
        )
        .route(
            "/api/pools/{pool_id}",
            axum::routing::delete(crate::dashboard::admin_api::admin_api_delete_pool),
        )
        .route(
            "/api/pools/{pool_id}/keys",
            axum::routing::post(crate::dashboard::admin_api::admin_api_add_pool_key),
        )
        .route(
            "/api/routing",
            axum::routing::get(crate::dashboard::admin_api::admin_api_list_routing)
                .post(crate::dashboard::admin_api::admin_api_create_routing),
        )
        .route(
            "/api/routing/{logical_model}",
            axum::routing::delete(crate::dashboard::admin_api::admin_api_delete_routing),
        )
        .route_layer(axum::middleware::from_fn(move |req, next| {
            middleware::ip_guard(req, next, allowed_ips.clone())
        }))
        .fallback_service(ServeDir::new("frontend/dist"));

    admin.with_state(state)
}

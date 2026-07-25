//! HTTP server layer: axum router, middleware chain, and request handlers (v2.0).
//!
//! Frontend served as static SPA from `frontend/dist/`. All Admin API routes
//! remain as REST + WebSocket endpoints. CORS enabled by default.

pub mod handler;
pub mod middleware;

use std::sync::Arc;

use axum::Router;
use dashmap::DashMap;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::alerts::{AlertEvent, AlertSnapshot};
use crate::auth;
use crate::budget::BudgetManager;
use crate::cache::PromptCache;
use crate::circuit_breaker::CircuitBreaker;
use crate::concurrency::ConcurrencyLimiter;
use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::metrics::Metrics;
use crate::rbac::middleware::RbacState;
use crate::router::RouterHandle;

#[derive(Clone)]
pub struct AppState {
    pub router: RouterHandle,
    pub db: sqlx::SqlitePool,
    pub config: Arc<Config>,
    pub cache: PromptCache,
    pub metrics: Arc<Metrics>,
    pub circuit_breaker: Arc<CircuitBreaker>,
    pub concurrency: Arc<ConcurrencyLimiter>,
    pub fallback_config: Arc<crate::fallback::FallbackConfig>,
    pub alert_tx: tokio::sync::broadcast::Sender<AlertEvent>,
    pub error_burst_counters: Arc<DashMap<(String, String), u32>>,
    pub alert_snapshot: AlertSnapshot,
    pub auth_store: Option<Arc<crate::auth_store::AuthStore>>,
    pub quota_tracker: Option<Arc<crate::quota::QuotaTracker>>,
    pub pipeline: Option<Arc<crate::pipeline::Pipeline>>,
    /// v3.0: RBAC state (Fix 1). None when RBAC is not configured.
    pub rbac_state: Option<RbacState>,
    /// v4.0 Track H: DB-backed config store for hot-reload and admin CRUD.
    pub config_store: Option<Arc<ConfigStore>>,
    /// v4.0 Track I: Budget manager for spend tracking.
    pub budget_manager: Option<Arc<BudgetManager>>,
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

    let api = if let Some(rl) = rate_limiter {
        api.route_layer(axum::middleware::from_fn_with_state(
            rl,
            crate::ratelimit::rate_limit_middleware,
        ))
    } else {
        api
    };

    let api = if let Some(qt) = &state.quota_tracker {
        api.route_layer(axum::middleware::from_fn_with_state(
            Arc::clone(qt),
            crate::quota::quota_middleware,
        ))
    } else {
        api
    };

    let api = api
        .layer(tower_http::limit::RequestBodyLimitLayer::new(limit))
        .route_layer(axum::middleware::from_fn_with_state(
            auth_state,
            auth::require_auth,
        ))
        .layer(axum::middleware::from_fn(middleware::request_id_middleware));

    let admin_allowed_ips = state.config.admin.allowed_ips.clone();

    let admin = build_admin_routes(state.clone(), admin_allowed_ips);

    let auth = build_auth_routes(state.clone());

    Router::new()
        .merge(auth.with_state(state.clone()))
        .merge(api.with_state(state.clone()))
        .nest("/admin", admin.with_state(state.clone()))
        .fallback_service(ServeDir::new("frontend/dist"))
        .layer(cors)
        .with_state(state)
}

fn build_admin_routes(state: AppState, allowed_ips: Vec<String>) -> Router<AppState> {
    let admin = Router::new()
        // User CRUD (T193) — protected by IP guard + RBAC
        .route(
            "/api/users",
            axum::routing::get(crate::dashboard::auth_api::user_list)
                .post(crate::dashboard::auth_api::user_create),
        )
        .route(
            "/api/users/{user_id}",
            axum::routing::patch(crate::dashboard::auth_api::user_update)
                .delete(crate::dashboard::auth_api::user_delete),
        )
        .route(
            "/",
            axum::routing::get(crate::dashboard::cost::cost_overview_handler),
        )
        .route(
            "/requests",
            axum::routing::get(crate::dashboard::requests::request_list_handler),
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
            "/help",
            axum::routing::get(crate::dashboard::help::help_handler),
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
            "/api/reload",
            axum::routing::post(crate::dashboard::admin_api::admin_api_reload),
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
            "/api/quotas",
            axum::routing::get(crate::dashboard::admin_api::admin_api_quotas),
        )
        .route("/ws", axum::routing::get(crate::dashboard::ws::ws_handler))
        .route(
            "/live",
            axum::routing::get(crate::dashboard::live::live_handler),
        )
        .route(
            "/export",
            axum::routing::get(crate::dashboard::export::export_handler),
        )
        .route(
            "/api/replay/{request_id}",
            axum::routing::get(crate::dashboard::replay::replay_request),
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
            "/api/routing",
            axum::routing::get(crate::dashboard::admin_api::admin_api_list_routing)
                .post(crate::dashboard::admin_api::admin_api_create_routing),
        )
        .route(
            "/api/routing/{logical_model}",
            axum::routing::delete(crate::dashboard::admin_api::admin_api_delete_routing),
        )
        .route(
            "/api/config/rollback/{audit_id}",
            axum::routing::post(crate::dashboard::admin_api::admin_api_rollback_config),
        )
        // ── v4.0 AUDIT-15 Fix: budget validation endpoint ──
        .route(
            "/api/validate-budget",
            axum::routing::post(crate::dashboard::admin_api::admin_api_validate_budget),
        )
        // ── v4.1 Audit-25 Fix: Stub endpoints for pending tracks ──
        .route(
            "/api/models",
            axum::routing::get(crate::dashboard::stub_api::models_list)
                .post(crate::dashboard::stub_api::models_create),
        )
        .route(
            "/api/models/{model_id}",
            axum::routing::patch(crate::dashboard::stub_api::models_update)
                .delete(crate::dashboard::stub_api::models_delete),
        )
        .route(
            "/api/routing/simulate",
            axum::routing::post(crate::dashboard::stub_api::routing_simulate),
        )
        .route(
            "/api/organizations",
            axum::routing::get(crate::dashboard::stub_api::orgs_list)
                .post(crate::dashboard::stub_api::orgs_create),
        )
        .route(
            "/api/organizations/{org_id}",
            axum::routing::patch(crate::dashboard::stub_api::orgs_update),
        )
        .route(
            "/api/budget/spend-history",
            axum::routing::post(crate::dashboard::stub_api::budget_spend_history),
        )
        .route(
            "/api/roles/{role_id}/permissions",
            axum::routing::post(crate::dashboard::stub_api::role_permissions),
        )
        .route(
            "/api/overview",
            axum::routing::get(crate::dashboard::stub_api::overview_aggregate),
        )
        .route(
            "/api/requests/search",
            axum::routing::post(crate::dashboard::stub_api::requests_search),
        )
        .route(
            "/api/alert-rules",
            axum::routing::get(crate::dashboard::stub_api::alert_rules_list)
                .post(crate::dashboard::stub_api::alert_rules_create),
        )
        .route(
            "/api/alert-rules/{rule_id}",
            axum::routing::patch(crate::dashboard::stub_api::alert_rules_update)
                .delete(crate::dashboard::stub_api::alert_rules_delete),
        )
        .route(
            "/api/alert-silences",
            axum::routing::get(crate::dashboard::stub_api::alert_silences_list)
                .post(crate::dashboard::stub_api::alert_silences_create),
        )
        .route(
            "/api/alert-silences/{silence_id}",
            axum::routing::delete(crate::dashboard::stub_api::alert_silences_delete),
        )
        .route(
            "/api/audit-logs",
            axum::routing::get(crate::dashboard::stub_api::audit_log_query),
        )
        .route(
            "/api/settings",
            axum::routing::get(crate::dashboard::stub_api::settings_get)
                .patch(crate::dashboard::stub_api::settings_update),
        )
        // ── IP guard remains as network-layer defense-in-depth ──
        // ── IP guard remains as network-layer defense-in-depth ──
        .route_layer(axum::middleware::from_fn(move |req, next| {
            middleware::ip_guard(req, next, allowed_ips.clone())
        }))
        .fallback_service(ServeDir::new("frontend/dist"));

    // ── v3.0 (Fix 1): RBAC authentication layer ──
    let mut admin = admin;
    let maybe_rbac = state.rbac_state.clone();
    if let Some(rbac_state) = maybe_rbac {
        admin = admin.route_layer(axum::middleware::from_fn_with_state(
            rbac_state,
            crate::rbac::middleware::rbac_middleware,
        ));
    }

    admin.with_state(state)
}

/// Auth routes — NO IP guard. Login/refresh are public (no RBAC),
/// /me is protected by RBAC middleware.
fn build_auth_routes(state: AppState) -> Router<AppState> {
    // Public routes — no RBAC (AUDIT-19 fix: login must be accessible without token)
    let public = Router::new()
        .route(
            "/api/auth/login",
            axum::routing::post(crate::dashboard::auth_api::auth_login),
        )
        .route(
            "/api/auth/refresh",
            axum::routing::post(crate::dashboard::auth_api::auth_refresh),
        );

    // Protected /me route with RBAC middleware
    let protected = Router::new().route(
        "/api/auth/me",
        axum::routing::get(crate::dashboard::auth_api::auth_me),
    );

    let protected = if let Some(rbac_state) = state.rbac_state.clone() {
        protected.route_layer(axum::middleware::from_fn_with_state(
            rbac_state,
            crate::rbac::middleware::rbac_middleware,
        ))
    } else {
        protected
    };

    public.merge(protected)
}

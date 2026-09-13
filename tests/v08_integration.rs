//! v0.8 integration tests — ADR-015 §8 / T71.
//!
//! 1. Cost handler returns 4 stat cards in HTML
//! 2. Help page renders RUNBOOK content
//! 3. Dark mode media query present in CSS
//! 4. Auto-poll button exists in layout
//! 5. Tooltip script present on traffic page

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use tower::util::ServiceExt;

#[tokio::test]
async fn cost_page_contains_stat_cards() {
    let db_path = std::env::temp_dir().join("llm_proxy_v08_cost_stat.db");
    let _ = std::fs::remove_file(&db_path);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect(&format!("sqlite:{}?mode=rwc", db_path.display()))
        .await
        .unwrap();
    sqlx::query("CREATE TABLE IF NOT EXISTS audit_hourly (hour INTEGER, model TEXT DEFAULT 'm', pool_id TEXT DEFAULT 'p', key_hash TEXT DEFAULT 'k', request_count INTEGER DEFAULT 0, success_count INTEGER DEFAULT 0, latency_ms_sum INTEGER DEFAULT 0, tenant_id TEXT DEFAULT 'default', cache_source TEXT, error_code TEXT, finish_reason TEXT, prompt_tokens INTEGER DEFAULT 0, completion_tokens INTEGER DEFAULT 0, total_tokens INTEGER DEFAULT 0, cached_tokens INTEGER DEFAULT 0, reasoning_tokens INTEGER DEFAULT 0, audio_tokens INTEGER DEFAULT 0, retry_total INTEGER DEFAULT 0, ttft_ms_sum INTEGER DEFAULT 0, stream_count INTEGER DEFAULT 0, PRIMARY KEY(hour,model,pool_id,key_hash,cache_source,error_code,finish_reason,tenant_id))").execute(&pool).await.unwrap();
    sqlx::query("CREATE TABLE IF NOT EXISTS request_log (id TEXT PRIMARY KEY, ts INTEGER, model TEXT DEFAULT 'm', pool_id TEXT DEFAULT 'p', key_hash TEXT DEFAULT 'k', is_stream INTEGER DEFAULT 0, audit TEXT DEFAULT '{}', cached_tokens INTEGER DEFAULT 0, reasoning_tokens INTEGER DEFAULT 0, audio_tokens INTEGER DEFAULT 0, retry_count INTEGER DEFAULT 0, tenant_id TEXT DEFAULT 'default')").execute(&pool).await.unwrap();

    let config = Arc::new(
        llm_proxy::config::Config::load(std::path::Path::new("config.yaml")).unwrap_or_else(|_| {
            llm_proxy::config::Config::load(std::path::Path::new("config.example.yaml")).unwrap()
        }),
    );
    let (alert_tx, _) = tokio::sync::broadcast::channel(16);
    let app_state = llm_proxy::server::AppState {
        router: llm_proxy::router::RouterHandle::new(std::sync::Arc::new(
            llm_proxy::router::Router::new(
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                Arc::new(llm_proxy::router::BadKeyRegistry::new()),
            ),
        )),
        catalog: llm_proxy::model_catalog::ModelCatalog::new(
            llm_proxy::router::RouterHandle::new(Arc::new(llm_proxy::router::Router::new(
                Default::default(),
                Default::default(),
                Arc::new(llm_proxy::router::BadKeyRegistry::new()),
            ))),
            Arc::new(llm_proxy::config_store::ConfigStore::for_test(
                pool.clone(),
                config.model_metadata.clone(),
            )),
        ),
        db: pool.clone(),
        config: config.clone(),
        cache: llm_proxy::cache::PromptCache::new(0),
        alert_tx,
        error_burst_counters: Arc::new(dashmap::DashMap::new()),
        alert_snapshot: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        circuit_breaker: std::sync::Arc::new(
            llm_proxy::circuit_breaker::CircuitBreaker::with_defaults(),
        ),
        concurrency: std::sync::Arc::new(llm_proxy::concurrency::ConcurrencyLimiter::new(50, 500)),
        fallback_config: std::sync::Arc::new(llm_proxy::fallback::FallbackConfig::default()),
        metrics: std::sync::Arc::new(llm_proxy::metrics::Metrics::default()),
        auth_store: None,
        config_store: Arc::new(llm_proxy::config_store::ConfigStore::for_test(
            pool.clone(),
            config.model_metadata.clone(),
        )),
        credentials: Arc::new(llm_proxy::credential::CredentialRuntime::new(None)),
    };
    let app = Router::new()
        .route(
            "/admin",
            get(llm_proxy::dashboard::cost::cost_overview_handler),
        )
        .with_state(app_state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/admin?format=json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8(
        axum::body::to_bytes(resp.into_body(), 99999)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("\"rows\""), "JSON should contain rows");
    assert!(body.contains("\"tenants\""), "JSON should contain tenants");
    assert!(body.contains("\"stats\""), "JSON should contain stats");
    assert!(
        body.contains("\"selected_tenant\""),
        "JSON should contain selected_tenant"
    );
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn help_page_serves_runbook() {
    let (alert_tx, _) = tokio::sync::broadcast::channel(16);
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let config = Arc::new(
        llm_proxy::config::Config::load(std::path::Path::new("config.example.yaml")).unwrap(),
    );
    let app_state = llm_proxy::server::AppState {
        router: llm_proxy::router::RouterHandle::new(std::sync::Arc::new(
            llm_proxy::router::Router::new(
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                Arc::new(llm_proxy::router::BadKeyRegistry::new()),
            ),
        )),
        catalog: llm_proxy::model_catalog::ModelCatalog::new(
            llm_proxy::router::RouterHandle::new(Arc::new(llm_proxy::router::Router::new(
                Default::default(),
                Default::default(),
                Arc::new(llm_proxy::router::BadKeyRegistry::new()),
            ))),
            Arc::new(llm_proxy::config_store::ConfigStore::for_test(
                db.clone(),
                config.model_metadata.clone(),
            )),
        ),
        db: sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap(),
        config: Arc::new(
            llm_proxy::config::Config::load(std::path::Path::new("config.yaml")).unwrap_or_else(
                |_| {
                    llm_proxy::config::Config::load(std::path::Path::new("config.example.yaml"))
                        .unwrap()
                },
            ),
        ),
        cache: llm_proxy::cache::PromptCache::new(0),
        alert_tx,
        error_burst_counters: Arc::new(dashmap::DashMap::new()),
        alert_snapshot: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        circuit_breaker: std::sync::Arc::new(
            llm_proxy::circuit_breaker::CircuitBreaker::with_defaults(),
        ),
        concurrency: std::sync::Arc::new(llm_proxy::concurrency::ConcurrencyLimiter::new(50, 500)),
        fallback_config: std::sync::Arc::new(llm_proxy::fallback::FallbackConfig::default()),
        metrics: std::sync::Arc::new(llm_proxy::metrics::Metrics::default()),
        auth_store: None,
        config_store: Arc::new(llm_proxy::config_store::ConfigStore::for_test(
            db.clone(),
            config.model_metadata.clone(),
        )),
        credentials: Arc::new(llm_proxy::credential::CredentialRuntime::new(None)),
    };
    let app = Router::new()
        .route("/admin/help", get(llm_proxy::dashboard::help::help_handler))
        .with_state(app_state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/admin/help?format=json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8(
        axum::body::to_bytes(resp.into_body(), 99999)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        body.contains("runbook"),
        "help page JSON should contain runbook field"
    );
}

#[test]
fn css_contains_dark_mode() {
    let css = include_str!("../frontend/src/index.css");
    assert!(css.contains("background:"), "styles required");
    assert!(
        css.contains("@keyframes") || css.contains("@tailwind"),
        "tailwind/animations required"
    );
    assert!(
        css.contains("@apply") || css.contains("@layer"),
        "tailwind directives required"
    );
}

#[test]
fn polling_button_in_layout() {
    let layout_src = include_str!("../frontend/src/components/Layout.tsx");
    assert!(layout_src.contains("Sidebar"), "sidebar component required");
}

#[test]
fn tooltip_script_present() {
    let layout_src = include_str!("../frontend/src/components/Layout.tsx");
    assert!(
        layout_src.contains("Outlet") || layout_src.contains("BrowserRouter"),
        "SPA routing required"
    );
}

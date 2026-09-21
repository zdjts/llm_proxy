//! Integration tests for the usage analytics API (`GET /admin/api/usage`).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use llm_proxy::config;
use llm_proxy::server::{self, AppState};
use sqlx::SqlitePool;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::broadcast;
use tower::ServiceExt;

async fn setup() -> (SqlitePool, TempDir, AppState) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test_usage.db");
    let path_str = db_path.to_str().unwrap();

    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path_str)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePool::connect_with(options).await.unwrap();

    let migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
        .await
        .unwrap();
    migrator.run(&pool).await.unwrap();

    let config = Arc::new(config::Config {
        server: config::ServerConfig {
            host: "127.0.0.1".into(),
            port: 4000,
            max_body_bytes: 10_485_760,
        },
        auth: config::AuthConfig {
            client_keys: vec![],
        },
        db: config::DbConfig {
            path: "./test.db".into(),
        },
        failover: config::FailoverConfig {
            enabled: true,
            bad_status_codes: vec![401, 402, 403, 429],
            max_retries: 1,
            probe_interval_secs: 60,
            probe_timeout_secs: 10,
            max_probe_retries: 3,
        },
        pools: Default::default(),
        providers: vec![],
        model_to_pool: Default::default(),
        model_registry: Default::default(),
        model_metadata: Default::default(),
        admin: config::AdminConfig {
            enabled: true,
            allowed_ips: vec!["127.0.0.1".into()],
        },
        pricing: Default::default(),
        rate_limit: Default::default(),
        cache_max_entries: 256,
        alerts: Default::default(),
        concurrency: Default::default(),
        response_normalization: Default::default(),
    });

    let (alert_tx, _) = broadcast::channel(16);
    let config_store = Arc::new(llm_proxy::config_store::ConfigStore::for_test(
        pool.clone(),
        config.model_metadata.clone(),
    ));
    let router = llm_proxy::router::RouterHandle::new(Arc::new(llm_proxy::router::Router::new(
        Default::default(),
        Default::default(),
        Arc::new(llm_proxy::router::BadKeyRegistry::new()),
    )));
    let state = AppState {
        router: router.clone(),
        catalog: llm_proxy::model_catalog::ModelCatalog::new(router, Arc::clone(&config_store)),
        config: Arc::clone(&config),
        db: pool.clone(),
        config_store,
        credentials: Arc::new(llm_proxy::credential::CredentialRuntime::new(None)),
        cache: llm_proxy::cache::PromptCache::new(0),
        metrics: Arc::new(llm_proxy::metrics::Metrics::default()),
        circuit_breaker: Arc::new(llm_proxy::circuit_breaker::CircuitBreaker::with_defaults()),
        concurrency: Arc::new(llm_proxy::concurrency::ConcurrencyLimiter::new(50, 500)),
        alert_tx,
        error_burst_counters: Arc::new(dashmap::DashMap::new()),
        alert_snapshot: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        auth_store: None,
    };

    (pool, dir, state)
}

fn app(state: AppState) -> axum::Router {
    server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    )
}

/// Seed one audit_hourly row inside the current UTC day.
async fn seed_row(pool: &SqlitePool, hour: i64, model: &str, requests: i64, cached: i64) {
    sqlx::query(
        "INSERT INTO audit_hourly (hour, model, pool_id, key_hash, cache_source, error_code, \
         finish_reason, upstream_model, tenant_id, request_count, success_count, \
         prompt_tokens, completion_tokens, total_tokens, cached_tokens, latency_ms_sum) \
         VALUES (?1, ?2, 'p1', 'hash1', '', '', 'stop', '', 'default', ?3, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(hour)
    .bind(model)
    .bind(requests)
    .bind(requests * 100)
    .bind(requests * 50)
    .bind(requests * 150)
    .bind(cached)
    .bind(requests * 200)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn usage_returns_summary_trend_and_model_breakdown() {
    let (pool, _dir, state) = setup().await;
    // `audit_hourly.hour` stores the epoch seconds of the hour bucket start.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let now_hour = (now / 3600) * 3600;
    seed_row(&pool, now_hour, "gpt-test", 10, 200).await;
    seed_row(&pool, now_hour - 48 * 3600, "gpt-test", 5, 0).await;

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/usage?hours=72")
                .header("x-real-ip", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"]["requests"], 15);
    assert_eq!(json["total"]["prompt_tokens"], 1500);
    assert_eq!(json["total"]["cached_tokens"], 200);
    assert_eq!(json["total"]["avg_latency_ms"], 200);
    assert_eq!(json["today"]["requests"], 10);
    assert_eq!(json["trend"].as_array().unwrap().len(), 2);
    assert_eq!(json["models"][0]["model"], "gpt-test");
    assert_eq!(json["models"][0]["requests"], 15);
    let hit = json["models"][0]["cache_hit_rate"].as_f64().unwrap();
    assert!((hit - 200.0 / 1500.0 * 100.0).abs() < 0.01);
}

#[tokio::test]
async fn usage_window_cost_is_not_zero_when_catalog_prices_exist() {
    // Regression: `UsageSummary.cost_usd` was hard-coded to 0.0 in
    // `summarize()`, so the window cost card always read $0.00 even while the
    // per-model rows showed real money. It also pins the pricing layering:
    // the only price source here is model_registry (layer 4) — there is no
    // YAML `pricing:` section and no pricing_override row.
    let (pool, _dir, state) = setup().await;

    sqlx::query(
        "INSERT INTO model_registry (id, display_name, provider_kind, \
         max_context_tokens, max_output_tokens, \
         input_price_per_1m, output_price_per_1m, enabled) \
         VALUES ('gpt-test', 'gpt-test', 'openai', 1000, 1000, 2.0, 8.0, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // The store must pick the catalog price up from the DB, exactly as the
    // running gateway does after `import-models`.
    state.config_store.refresh_from_db().await.unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let now_hour = (now / 3600) * 3600;
    // 10 requests x (100 prompt, 50 completion) = 1000 prompt, 500 completion.
    seed_row(&pool, now_hour, "gpt-test", 10, 0).await;

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/usage?hours=24")
                .header("x-real-ip", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // 1000/1M * 2.0 + 500/1M * 8.0 = 0.002 + 0.004 = 0.006
    let expected = 0.006;
    let total_cost = json["total"]["cost_usd"].as_f64().unwrap();
    assert!(
        (total_cost - expected).abs() < 1e-9,
        "window cost must aggregate model costs, got {total_cost}, expected {expected}"
    );
    let today_cost = json["today"]["cost_usd"].as_f64().unwrap();
    assert!(
        (today_cost - expected).abs() < 1e-9,
        "today cost must match, got {today_cost}"
    );
    let trend_cost: f64 = json["trend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["cost_usd"].as_f64().unwrap())
        .sum();
    assert!(
        (trend_cost - expected).abs() < 1e-9,
        "trend cost must sum to the same total, got {trend_cost}"
    );
}

#[tokio::test]
async fn usage_tenant_filter_excludes_other_tenants() {
    let (pool, _dir, state) = setup().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let now_hour = (now / 3600) * 3600;
    seed_row(&pool, now_hour, "gpt-test", 10, 0).await;
    sqlx::query(
        "INSERT INTO audit_hourly (hour, model, pool_id, key_hash, cache_source, error_code, \
         finish_reason, upstream_model, tenant_id, request_count, success_count) \
         VALUES (?1, 'gpt-test', 'p1', 'h', '', '', 'stop', '', 'acme', 7, 7)",
    )
    .bind(now_hour)
    .execute(&pool)
    .await
    .unwrap();

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/usage?hours=24&tenant=acme")
                .header("x-real-ip", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"]["requests"], 7);
    assert_eq!(json["selected_tenant"], "acme");
}

#[tokio::test]
async fn usage_is_hidden_from_non_whitelisted_ips() {
    let (_pool, _dir, state) = setup().await;
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/usage")
                .header("x-real-ip", "192.0.2.10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

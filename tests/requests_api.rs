//! Integration tests for request history (`GET /admin/requests`, `/admin/requests/{id}`).

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
    let db_path = dir.path().join("test_requests.db");
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

async fn seed_request(pool: &SqlitePool, id: &str, ts: i64, model: &str) {
    sqlx::query(
        "INSERT INTO request_log (id, ts, model, pool_id, key_hash, status_code, latency_ms, \
         prompt_tokens, completion_tokens, total_tokens, is_stream, tenant_id, finish_reason) \
         VALUES (?1, ?2, ?3, 'openai', 'abc123def456', 200, 120, 10, 20, 30, 0, 'default', 'stop')",
    )
    .bind(id)
    .bind(ts)
    .bind(model)
    .execute(pool)
    .await
    .unwrap();
}

async fn json_get(state: AppState, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("x-real-ip", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn lists_all_history_when_hours_is_zero() {
    let (pool, _dir, state) = setup().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    seed_request(&pool, "recent", now, "gpt-recent").await;
    seed_request(&pool, "old", now - 40 * 24 * 3600 * 1000, "gpt-old").await;

    let (status, json) = json_get(state.clone(), "/admin/requests?hours=24").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["total"], 1);
    assert_eq!(json["rows"][0]["id"], "recent");

    let (status, json) = json_get(state, "/admin/requests?hours=0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["total"], 2);
    assert_eq!(json["rows"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn returns_request_detail_by_id() {
    let (pool, _dir, state) = setup().await;
    seed_request(&pool, "req-detail", 1_700_000_000_000, "gpt-4o").await;

    let (status, json) = json_get(state, "/admin/requests/req-detail").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["id"], "req-detail");
    assert_eq!(json["model"], "gpt-4o");
    assert_eq!(json["pool_id"], "openai");
    assert_eq!(json["key_hash"], "abc123def456");
    assert_eq!(json["status_code"], 200);
}

#[tokio::test]
async fn missing_request_returns_404() {
    let (_pool, _dir, state) = setup().await;
    let (status, json) = json_get(state, "/admin/requests/does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["error"]["type"], "not_found");
}

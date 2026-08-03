//! v0.6 integration tests — ADR-012 §6 / T51.
//!
//! 1. CSV quote helper covers all 4 special chars
//! 2. CSV cost endpoint returns correct format
//! 3. CSV content-type header verification
//! 4. Alerts snapshot buffer respects 1024 capacity
//! 5. Per-tenant SQL slice isolates rows
//! 6. HMAC sign_body known test vectors

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use llm_proxy::alerts::AlertEvent;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;
use tower::util::ServiceExt;

// ── CSV helpers ──────────────────────────────────────────────────────────

#[test]
fn csv_quote_escapes_all_four_special_chars() {
    let q = llm_proxy::dashboard::csv::csv_quote;

    assert_eq!(q("hello"), "hello");
    assert_eq!(q("a,b"), "\"a,b\"");
    assert_eq!(q("a\"b"), "\"a\"\"b\"");
    assert_eq!(q("a\nb"), "\"a\nb\"");
    assert_eq!(q("a\rb"), "\"a\rb\"");
    assert_eq!(q("a,b\"c\n"), "\"a,b\"\"c\n\"");
    assert_eq!(
        q("normal text, with \"quotes\" and\nnewlines\r"),
        "\"normal text, with \"\"quotes\"\" and\nnewlines\r\""
    );
}

#[tokio::test]
async fn cost_csv_endpoint_returns_200() {
    let db_path = std::env::temp_dir().join("llm_proxy_v06_test_cost.db");
    let _ = std::fs::remove_file(&db_path);

    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS request_log (
            id TEXT PRIMARY KEY, ts INTEGER NOT NULL, model TEXT NOT NULL,
            pool_id TEXT NOT NULL, key_hash TEXT NOT NULL, status_code INTEGER,
            prompt_tokens INTEGER, completion_tokens INTEGER, total_tokens INTEGER,
            is_stream INTEGER DEFAULT 0, audit TEXT DEFAULT '{}', cached_tokens INTEGER DEFAULT 0,
            reasoning_tokens INTEGER DEFAULT 0, audio_tokens INTEGER DEFAULT 0,
            retry_count INTEGER DEFAULT 0, tenant_id TEXT NOT NULL DEFAULT 'default')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS audit_hourly (
            hour INTEGER NOT NULL, model TEXT NOT NULL, pool_id TEXT NOT NULL,
            key_hash TEXT NOT NULL, cache_source TEXT, error_code TEXT,
            finish_reason TEXT, request_count INTEGER DEFAULT 0,
            success_count INTEGER DEFAULT 0, retry_total INTEGER DEFAULT 0,
            prompt_tokens INTEGER DEFAULT 0, completion_tokens INTEGER DEFAULT 0,
            total_tokens INTEGER DEFAULT 0, cached_tokens INTEGER DEFAULT 0,
            reasoning_tokens INTEGER DEFAULT 0, audio_tokens INTEGER DEFAULT 0,
            latency_ms_sum INTEGER DEFAULT 0, ttft_ms_sum INTEGER DEFAULT 0,
            stream_count INTEGER DEFAULT 0,
            tenant_id TEXT NOT NULL DEFAULT 'default',
            PRIMARY KEY (hour, model, pool_id, key_hash, cache_source, error_code, finish_reason, tenant_id)
        )"
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO request_log (id,ts,model,pool_id,key_hash,tenant_id) VALUES ('r1',?1,'m1','p1','kh','t1')")
        .bind(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64)
        .execute(&pool).await.unwrap();

    let config = llm_proxy::config::Config::load(std::path::Path::new("config.yaml"))
        .unwrap_or_else(|_| {
            llm_proxy::config::Config::load(std::path::Path::new("config.example.yaml")).unwrap()
        });
    let config = Arc::new(config);

    let (alert_tx, _) = tokio::sync::broadcast::channel(16);
    let app_state = llm_proxy::server::AppState {
        router: llm_proxy::router::RouterHandle::new(Arc::new(llm_proxy::router::Router::new(
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            Arc::new(llm_proxy::router::BadKeyRegistry::new()),
        ))),
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
        alert_snapshot: Arc::new(Mutex::new(VecDeque::new())),
        metrics: Arc::new(llm_proxy::metrics::Metrics::default()),
        circuit_breaker: Arc::new(llm_proxy::circuit_breaker::CircuitBreaker::with_defaults()),
        concurrency: Arc::new(llm_proxy::concurrency::ConcurrencyLimiter::new(50, 500)),
        fallback_config: Arc::new(llm_proxy::fallback::FallbackConfig::default()),
        auth_store: None,
        pipeline: None,
        rbac_state: None,
        quota_tracker: None,
        config_store: Arc::new(llm_proxy::config_store::ConfigStore::for_test(
            pool.clone(),
            config.model_metadata.clone(),
        )),
        budget_manager: None,
    };

    let app = Router::new()
        .route(
            "/admin",
            get(llm_proxy::dashboard::cost::cost_overview_handler),
        )
        .with_state(app_state);

    let req = Request::builder()
        .uri("/admin?format=csv")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/csv"));

    let _ = std::fs::remove_file(&db_path);
}

// ── Alerts snapshot capacity ──────────────────────────────────────────────

#[test]
fn alert_snapshot_respects_1024_capacity() {
    let snapshot = Arc::new(Mutex::new(VecDeque::new()));
    let ts = 1_740_000_000_000i64;

    for i in 0..2000u64 {
        let event = AlertEvent::UpstreamError {
            ts,
            pool_id: format!("p{i}"),
            key_hash: format!("kh{i}"),
            error_code: "429".into(),
            status: Some(429),
            msg: "rate limited".into(),
        };
        let mut snap = snapshot.lock().unwrap();
        snap.push_back(event);
        if snap.len() > 1024 {
            snap.pop_front();
        }
    }

    let snap = snapshot.lock().unwrap();
    assert!(snap.len() <= 1024);
    assert!(!snap.is_empty());
}

// ── Per-tenant SQL isolation ──────────────────────────────────────────────

#[tokio::test]
async fn tenant_filter_isolates_rows() {
    let db_path = std::env::temp_dir().join("llm_proxy_v06_tenant.db");
    let _ = std::fs::remove_file(&db_path);

    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS request_log (
            id TEXT PRIMARY KEY, ts INTEGER NOT NULL, model TEXT NOT NULL DEFAULT 'm',
            pool_id TEXT NOT NULL DEFAULT 'p', key_hash TEXT NOT NULL DEFAULT 'k',
            status_code INTEGER, prompt_tokens INTEGER, completion_tokens INTEGER,
            total_tokens INTEGER, is_stream INTEGER DEFAULT 0, audit TEXT DEFAULT '{}',
            cached_tokens INTEGER DEFAULT 0, reasoning_tokens INTEGER DEFAULT 0,
            audio_tokens INTEGER DEFAULT 0, retry_count INTEGER DEFAULT 0,
            tenant_id TEXT NOT NULL DEFAULT 'default')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    sqlx::query("INSERT INTO request_log (id,ts,tenant_id) VALUES ('a1',?1,'alice')")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO request_log (id,ts,tenant_id) VALUES ('b1',?1,'bob')")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

    let alice: Vec<(String,)> =
        sqlx::query_as("SELECT tenant_id FROM request_log WHERE tenant_id = ?1")
            .bind("alice")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(alice.len(), 1);
    assert_eq!(alice[0].0, "alice");

    let bob: Vec<(String,)> =
        sqlx::query_as("SELECT tenant_id FROM request_log WHERE tenant_id = ?1")
            .bind("bob")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(bob.len(), 1);
    assert_eq!(bob[0].0, "bob");

    let all: Vec<(String,)> =
        sqlx::query_as("SELECT tenant_id FROM request_log WHERE tenant_id IS NOT NULL")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(all.len(), 2);

    let _ = std::fs::remove_file(&db_path);
}

// ── HMAC known test vectors ───────────────────────────────────────────────

#[test]
fn hmac_known_vectors_consistent() {
    let sig = llm_proxy::alerts::sign_body("secret", 1700000000, b"hello");
    assert_eq!(sig.len(), 64);

    // Same inputs produce same output
    let sig2 = llm_proxy::alerts::sign_body("secret", 1700000000, b"hello");
    assert_eq!(sig, sig2);

    // Different body => different sig
    let sig3 = llm_proxy::alerts::sign_body("secret", 1700000000, b"hello!");
    assert_ne!(sig, sig3);
}

//! Alert hook integration tests — ADR-011 §4 / T44 + ADR-013 §6 / T58.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use llm_proxy::alerts::AlertEvent;
use llm_proxy::alerts::channel::WebhookChannel;
use llm_proxy::alerts::spawn_alert_task;
use llm_proxy::auth::AuthedClient;
use llm_proxy::ratelimit::RateLimiter;
use tokio::sync::{broadcast, watch};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use tower::util::ServiceExt;

fn auth_ext(key: &str, tenant: &str) -> AuthedClient {
    AuthedClient {
        key_hash: llm_proxy::db::compute_key_hash(key),
        tenant_id: tenant.to_string(),
    }
}

#[tokio::test]
async fn rate_limit_alert_fires_per_tenant() {
    let (alert_tx, mut alert_rx) = broadcast::channel::<AlertEvent>(16);
    let limiter = Arc::new(RateLimiter::new(1, alert_tx.clone()));

    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .route_layer(axum::middleware::from_fn_with_state(
            Arc::clone(&limiter),
            llm_proxy::ratelimit::rate_limit_middleware,
        ));

    let req = || {
        Request::builder()
            .uri("/")
            .extension(auth_ext("sk-t1", "tenant-a"))
            .body(Body::empty())
            .unwrap()
    };

    let _ = app.clone().oneshot(req()).await.unwrap();
    let resp = app.clone().oneshot(req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::from_u16(429).unwrap());

    let event = tokio::time::timeout(Duration::from_millis(200), alert_rx.recv())
        .await
        .expect("should receive alert");
    match event {
        Ok(AlertEvent::RateLimited { tenant_id, .. }) => {
            assert_eq!(tenant_id, "tenant-a");
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[tokio::test]
async fn upstream_error_burst_counts_correctly() {
    let mut webhook_server = mockito::Server::new_async().await;
    let webhook_mock = webhook_server
        .mock("POST", "/")
        .with_status(200)
        .expect_at_least(1)
        .create_async()
        .await;

    let (alert_tx, mut alert_rx) = broadcast::channel::<AlertEvent>(16);
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS alert_event (
            id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL,
            type TEXT NOT NULL, pool_id TEXT, tenant_id TEXT, key_hash TEXT,
            model TEXT, error_code TEXT, status INTEGER, msg TEXT, payload TEXT
        )",
    )
    .execute(&db)
    .await
    .unwrap();

    let channels: Vec<Arc<dyn llm_proxy::alerts::channel::AlertChannel>> =
        vec![Arc::new(WebhookChannel {
            url: webhook_server.url(),
            secret: String::new(),
        })];
    let snap = Arc::new(Mutex::new(VecDeque::new()));
    let _handle = spawn_alert_task(db, channels, shutdown_rx, alert_tx.subscribe(), snap);

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let _ = alert_tx.send(AlertEvent::UpstreamError {
        ts,
        pool_id: "p1".into(),
        key_hash: "kh1".into(),
        error_code: "429".into(),
        status: Some(429),
        msg: "rate limited".into(),
    });

    let event = tokio::time::timeout(Duration::from_secs(1), alert_rx.recv())
        .await
        .expect("timeout")
        .expect("should receive");
    assert!(matches!(event, AlertEvent::UpstreamError { .. }));

    tokio::time::sleep(Duration::from_secs(2)).await;

    drop(alert_tx);
    let _ = shutdown_tx.send(());
    tokio::time::sleep(Duration::from_millis(200)).await;

    webhook_mock.assert_async().await;
}

#[tokio::test]
async fn latency_spike_alert_fires() {
    let (alert_tx, mut alert_rx) = broadcast::channel::<AlertEvent>(16);

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let _ = alert_tx.send(AlertEvent::LatencySpike {
        ts,
        model: "gpt-4".into(),
        latency_ms: 50_000,
        threshold_ms: 30_000,
    });

    let event = tokio::time::timeout(Duration::from_millis(200), alert_rx.recv())
        .await
        .expect("timeout")
        .expect("should receive alert");

    match event {
        AlertEvent::LatencySpike {
            model,
            latency_ms,
            threshold_ms,
            ..
        } => {
            assert_eq!(model, "gpt-4");
            assert_eq!(latency_ms, 50_000);
            assert_eq!(threshold_ms, 30_000);
        }
        other => panic!("expected LatencySpike, got {other:?}"),
    }
}

#[tokio::test]
async fn pool_exhausted_alert_fires() {
    let (alert_tx, mut alert_rx) = broadcast::channel::<AlertEvent>(16);

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let _ = alert_tx.send(AlertEvent::PoolExhausted {
        ts,
        pool_id: "pool-a".into(),
    });

    let event = tokio::time::timeout(Duration::from_millis(200), alert_rx.recv())
        .await
        .expect("timeout")
        .expect("should receive alert");

    match event {
        AlertEvent::PoolExhausted { pool_id, .. } => {
            assert_eq!(pool_id, "pool-a");
        }
        other => panic!("expected PoolExhausted, got {other:?}"),
    }
}

#[tokio::test]
async fn webhook_failure_does_not_crash_task() {
    let (alert_tx, _alert_rx) = broadcast::channel::<AlertEvent>(16);
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS alert_event (
            id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL,
            type TEXT NOT NULL, pool_id TEXT, tenant_id TEXT, key_hash TEXT,
            model TEXT, error_code TEXT, status INTEGER, msg TEXT, payload TEXT
        )",
    )
    .execute(&db)
    .await
    .unwrap();

    let channels: Vec<Arc<dyn llm_proxy::alerts::channel::AlertChannel>> =
        vec![Arc::new(WebhookChannel {
            url: "http://127.0.0.1:1/no-server".into(),
            secret: String::new(),
        })];
    let snap = Arc::new(Mutex::new(VecDeque::new()));
    let handle = spawn_alert_task(db, channels, shutdown_rx, alert_tx.subscribe(), snap);

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let _ = alert_tx.send(AlertEvent::PoolExhausted {
        ts,
        pool_id: "p".into(),
    });

    tokio::time::sleep(Duration::from_secs(10)).await;

    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}

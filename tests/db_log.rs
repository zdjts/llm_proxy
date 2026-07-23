//! Integration tests for the database log pipeline.
//!
//! Exercises `db::connect()`, `db::log_request()`, and verifies SQL invariants:
//! `total_tokens == prompt + completion`, `key_hash` length 12 and not plaintext,
//! nullable fields store `None` correctly.

use llm_proxy::audit::AuditDetail;
use llm_proxy::db::{self, RequestLog};
use sqlx::Row;
use tempfile::TempDir;

async fn setup() -> (sqlx::SqlitePool, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.db");
    let pool = db::connect(path.to_str().unwrap()).await.unwrap();
    (pool, dir)
}

#[tokio::test]
async fn log_request_writes_one_row() {
    let (pool, _dir) = setup().await;

    let id = uuid::Uuid::new_v4().to_string();
    let log = RequestLog {
        id: id.clone(),
        ts: 1,
        client_ip: Some("10.0.0.1".into()),
        model: "gpt-4o".into(),
        pool_id: "pool1".into(),
        key_hash: db::compute_key_hash("sk-abc123"),
        upstream: Some("https://api.openai.com/v1".into()),
        status_code: Some(200),
        latency_ms: Some(100),
        prompt_tokens: Some(50),
        completion_tokens: Some(30),
        total_tokens: Some(80),
        is_stream: false,
        error: None,
        audit: AuditDetail::none(),
        error_code: None,
    };

    db::log_request(&pool, &log).await.unwrap();

    let row = sqlx::query("SELECT COUNT(*) AS cnt FROM request_log")
        .fetch_one(&pool)
        .await
        .unwrap();
    let cnt: i64 = row.get("cnt");
    assert_eq!(cnt, 1);
}

#[tokio::test]
async fn total_tokens_equals_prompt_plus_completion() {
    let (pool, _dir) = setup().await;

    let id = uuid::Uuid::new_v4().to_string();
    let log = RequestLog {
        id: id.clone(),
        ts: 2,
        client_ip: None,
        model: "m".into(),
        pool_id: "p".into(),
        key_hash: db::compute_key_hash("k"),
        upstream: None,
        status_code: Some(200),
        latency_ms: Some(10),
        prompt_tokens: Some(60),
        completion_tokens: Some(40),
        total_tokens: Some(100),
        is_stream: false,
        error: None,
        audit: AuditDetail::none(),
        error_code: None,
    };

    db::log_request(&pool, &log).await.unwrap();

    let row = sqlx::query("SELECT * FROM request_log WHERE id = ?1")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();

    let prompt: i32 = row.get("prompt_tokens");
    let completion: i32 = row.get("completion_tokens");
    let total: i32 = row.get("total_tokens");
    assert_eq!(total, prompt + completion);
    assert_eq!(total, 100);
}

#[tokio::test]
async fn key_hash_is_12_hex_and_not_plaintext() {
    let kh = db::compute_key_hash("sk-secret-key-abc");
    assert_eq!(kh.len(), 12);
    assert!(kh.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(!kh.contains("secret"));
    assert!(!kh.contains("abc"));
}

#[tokio::test]
async fn nullable_fields_store_none() {
    let (pool, _dir) = setup().await;

    let id = uuid::Uuid::new_v4().to_string();
    let log = RequestLog {
        id: id.clone(),
        ts: 3,
        client_ip: None,
        model: "m".into(),
        pool_id: "p".into(),
        key_hash: db::compute_key_hash("k"),
        upstream: None,
        status_code: None,
        latency_ms: None,
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        is_stream: true,
        error: None,
        audit: AuditDetail::none(),
        error_code: None,
    };

    db::log_request(&pool, &log).await.unwrap();

    let row = sqlx::query("SELECT * FROM request_log WHERE id = ?1")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();

    let client_ip: Option<String> = row.get("client_ip");
    assert!(client_ip.is_none());
}

#[tokio::test]
async fn error_row_has_non_empty_error_field() {
    let (pool, _dir) = setup().await;

    let id = uuid::Uuid::new_v4().to_string();
    let log = RequestLog {
        id: id.clone(),
        ts: 4,
        client_ip: None,
        model: "m".into(),
        pool_id: "p".into(),
        key_hash: db::compute_key_hash("k"),
        upstream: None,
        status_code: Some(500),
        latency_ms: Some(1),
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        is_stream: false,
        error: Some("upstream timeout".into()),
        audit: AuditDetail::none(),
        error_code: None,
    };

    db::log_request(&pool, &log).await.unwrap();

    let row = sqlx::query("SELECT error FROM request_log WHERE id = ?1")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();

    let error: Option<String> = row.get("error");
    assert_eq!(error, Some("upstream timeout".into()));
}

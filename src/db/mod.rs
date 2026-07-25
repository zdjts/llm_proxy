//! SQLite persistence layer for per-request structured logging.
//!
//! Uses `sqlx` with runtime migration execution. Every completed chat request
//! writes one row to `request_log` with a hashed key identifier (SHA-256 first
//! 12 hex — never plaintext).

use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use crate::audit::{AuditDetail, ProviderCacheKind};
use crate::error::AppError;

/// Compute a safe identifier for an API key: SHA-256, first 12 hex characters.
pub fn compute_key_hash(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Open a SQLite connection pool with WAL mode enabled, then run pending
/// migrations.
pub async fn connect(path: &str) -> Result<SqlitePool, AppError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePool::connect_with(options)
        .await
        .map_err(|e| AppError::Config(format!("Failed to connect to SQLite at {path}: {e}")))?;

    let migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create migrator: {e}")))?;
    migrator
        .run(&pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to run migrations: {e}")))?;

    Ok(pool)
}

/// A single row for the `request_log` table. Fields mirror `migrations/0001_init.sql`;
/// nullable SQL columns use `Option<T>`.
#[derive(Debug)]
pub struct RequestLog {
    pub id: String,
    pub ts: i64,
    pub client_ip: Option<String>,
    pub model: String,
    pub pool_id: String,
    pub key_hash: String,
    pub upstream: Option<String>,
    pub status_code: Option<i16>,
    pub latency_ms: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub is_stream: bool,
    pub error: Option<String>,
    pub audit: AuditDetail,
    pub error_code: Option<String>,
    pub user_agent: Option<String>,
    pub cost_usd: Option<f64>,
}

/// Insert a log entry into the database. Async — caller must not block on the
/// request path.
pub async fn log_request(pool: &SqlitePool, log: &RequestLog) -> Result<(), AppError> {
    let cache_source = match log.audit.from_provider.cache.source {
        ProviderCacheKind::None => None,
        ref other => Some(other.to_string()),
    };

    sqlx::query(
        "INSERT INTO request_log \
         (id, ts, client_ip, model, pool_id, key_hash, upstream, \
          status_code, latency_ms, prompt_tokens, completion_tokens, \
          total_tokens, is_stream, error, \
          cached_tokens, cache_creation_tokens, cache_source, \
          reasoning_tokens, audio_tokens, \
          ttft_ms, upstream_model, system_fingerprint, finish_reason, \
          error_code, retry_count, tenant_id, user_agent, cost_usd) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28)",
    )
    .bind(&log.id)
    .bind(log.ts)
    .bind(&log.client_ip)
    .bind(&log.model)
    .bind(&log.pool_id)
    .bind(&log.key_hash)
    .bind(&log.upstream)
    .bind(log.status_code)
    .bind(log.latency_ms)
    .bind(log.prompt_tokens)
    .bind(log.completion_tokens)
    .bind(log.total_tokens)
    .bind(log.is_stream as i32)
    .bind(&log.error)
    .bind(log.audit.from_provider.cache.hit_tokens)
    .bind(log.audit.from_provider.cache.creation_tokens)
    .bind(cache_source)
    .bind(log.audit.from_provider.reasoning_tokens)
    .bind(log.audit.from_provider.audio_tokens)
    .bind(log.audit.from_router.ttft_ms)
    .bind(&log.audit.from_provider.upstream_model)
    .bind(&log.audit.from_provider.system_fingerprint)
    .bind(&log.audit.from_provider.finish_reason)
    .bind(&log.error_code)
    .bind(log.audit.from_router.retry_count)
    .bind(&log.audit.from_auth.tenant_id)
    .bind(&log.user_agent)
    .bind(log.cost_usd)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(format!("Failed to log request: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;
    use tempfile::TempDir;

    fn hashed(s: &str) -> String {
        compute_key_hash(s)
    }

    #[test]
    fn it_hashes_key_to_12_hex_chars() {
        let h = hashed("sk-test-key");
        assert_eq!(h.len(), 12);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));

        let h2 = hashed("sk-test-key");
        assert_eq!(h, h2, "same key must produce same hash");
    }

    #[test]
    fn it_produces_different_hashes_for_different_keys() {
        assert_ne!(hashed("key-a"), hashed("key-b"));
    }

    #[test]
    fn it_does_not_contain_plaintext_key() {
        let h = hashed("sk-secret-abc123");
        assert!(!h.contains("sk-secret"));
        assert!(!h.contains("abc123"));
    }

    async fn setup_test_pool() -> (SqlitePool, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_str().unwrap();

        let pool = connect(path_str).await.unwrap();
        (pool, dir)
    }

    #[tokio::test]
    async fn it_creates_pool_and_runs_migrations() {
        let (pool, _dir) = setup_test_pool().await;

        let tables: Vec<String> =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='request_log'")
                .fetch_all(&pool)
                .await
                .unwrap()
                .iter()
                .map(|r| r.get(0))
                .collect();

        assert!(tables.contains(&"request_log".to_string()));
    }

    #[tokio::test]
    async fn audit_hourly_dimensions_are_non_null_and_include_cost_fields() {
        let (pool, _dir) = setup_test_pool().await;

        let columns = sqlx::query("PRAGMA table_info(audit_hourly)")
            .fetch_all(&pool)
            .await
            .unwrap();
        let column_names: Vec<String> = columns.iter().map(|row| row.get("name")).collect();
        assert!(column_names.contains(&"upstream_model".to_string()));
        assert!(column_names.contains(&"cost_usd".to_string()));

        let cache_source_not_null: i64 = columns
            .iter()
            .find(|row| row.get::<String, _>("name") == "cache_source")
            .map(|row| row.get("notnull"))
            .unwrap();
        assert_eq!(cache_source_not_null, 1);

        let indexes = sqlx::query("PRAGMA index_list(audit_hourly)")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(indexes.iter().any(|row| {
            let name: String = row.get("name");
            let unique: i64 = row.get("unique");
            unique == 1 && name.starts_with("sqlite_autoindex_audit_hourly")
        }));
    }
    #[tokio::test]
    async fn it_logs_a_request() {
        let (pool, _dir) = setup_test_pool().await;

        let id = uuid::Uuid::new_v4().to_string();
        let entry = RequestLog {
            id: id.clone(),
            ts: 1234567890,
            client_ip: Some("127.0.0.1".into()),
            model: "gpt-4o".into(),
            pool_id: "openai_pool".into(),
            key_hash: compute_key_hash("sk-abc"),
            upstream: Some("https://api.openai.com/v1".into()),
            status_code: Some(200),
            latency_ms: Some(42),
            prompt_tokens: Some(10),
            completion_tokens: Some(20),
            total_tokens: Some(30),
            is_stream: false,
            error: None,
            audit: AuditDetail::none(),
            error_code: None,
            user_agent: None,
            cost_usd: None,
        };

        log_request(&pool, &entry).await.unwrap();

        let row = sqlx::query("SELECT * FROM request_log WHERE id = ?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();

        let db_id: String = row.get("id");
        assert_eq!(db_id, id);

        let db_total: i32 = row.get("total_tokens");
        let db_prompt: i32 = row.get("prompt_tokens");
        let db_completion: i32 = row.get("completion_tokens");
        assert_eq!(db_total, db_prompt + db_completion);
        assert_eq!(db_total, 30);

        let db_key_hash: String = row.get("key_hash");
        assert!(!db_key_hash.contains("sk-abc"), "key_hash must be hashed");
        assert_eq!(db_key_hash, compute_key_hash("sk-abc"));
    }

    #[tokio::test]
    async fn it_logs_nullable_fields_as_none() {
        let (pool, _dir) = setup_test_pool().await;

        let id = uuid::Uuid::new_v4().to_string();
        let entry = RequestLog {
            id: id.clone(),
            ts: 1,
            client_ip: None,
            model: "m".into(),
            pool_id: "p".into(),
            key_hash: compute_key_hash("k"),
            upstream: None,
            status_code: None,
            latency_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            is_stream: true,
            error: Some("something went wrong".into()),
            audit: AuditDetail::none(),
            error_code: None,
            user_agent: None,
            cost_usd: None,
        };

        log_request(&pool, &entry).await.unwrap();

        let row = sqlx::query("SELECT * FROM request_log WHERE id = ?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();

        let client_ip: Option<String> = row.get("client_ip");
        assert!(client_ip.is_none());

        let error: Option<String> = row.get("error");
        assert_eq!(error, Some("something went wrong".into()));

        let is_stream: i32 = row.get("is_stream");
        assert_eq!(is_stream, 1);
    }
}

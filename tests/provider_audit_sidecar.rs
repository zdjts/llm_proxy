//! Cache-audit integration tests (ADR-005 v2 / T14a).
//!
//! Verifies that `OpenAiProvider::extract_audit` correctly identifies
//! provider-specific cache fields and that the full `AuditDetail` round-trips
//! through `log_request` into SQLite.

use std::sync::Arc;

use llm_proxy::audit::{
    AuditDetail, AuditFromAuth, AuditFromProvider, AuditFromRouter, CacheReport, ErrorCode,
    ProviderCacheKind,
};
use llm_proxy::config::KeyEntry;
use llm_proxy::db::{self, RequestLog};
use llm_proxy::provider::openai::OpenAiProvider;
use llm_proxy::provider::{Provider, ProviderResponse};
use llm_proxy::types::ChatCompletionRequest;
use sqlx::Row;
use tempfile::TempDir;

async fn setup_db() -> (sqlx::SqlitePool, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.db");
    let pool = db::connect(path.to_str().unwrap()).await.unwrap();
    (pool, dir)
}

fn test_provider() -> OpenAiProvider {
    let codes: Arc<[u16]> = Arc::from([401, 402, 403, 429]);
    OpenAiProvider::new("test".into(), "https://test.local/v1".into(), codes)
}

// ── extract_audit unit tests ────────────────────────────────────────────

#[test]
fn openai_cached_tokens_are_detected() {
    let raw = serde_json::json!({
        "prompt_tokens": 100,
        "completion_tokens": 50,
        "total_tokens": 150,
        "prompt_tokens_details": { "cached_tokens": 800 }
    });

    let p = test_provider();
    let resp = llm_proxy::types::ChatCompletionResponse {
        id: "1".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
        raw_usage_json: Some(raw),
    };

    let audit = p.extract_audit(&ProviderResponse::Once(resp));
    assert_eq!(audit.cache.hit_tokens, Some(800));
    assert_eq!(audit.cache.source, ProviderCacheKind::OpenAiPromptCache);
}

#[test]
fn deepseek_cache_hit_tokens_are_detected() {
    let raw = serde_json::json!({
        "prompt_cache_hit_tokens": 500
    });

    let p = test_provider();
    let resp = llm_proxy::types::ChatCompletionResponse {
        id: "2".into(),
        object: "chat.completion".into(),
        created: 1,
        model: "deepseek-chat".into(),
        choices: vec![],
        usage: None,
        raw_usage_json: Some(raw),
    };

    let audit = p.extract_audit(&ProviderResponse::Once(resp));
    assert_eq!(audit.cache.hit_tokens, Some(500));
    assert_eq!(audit.cache.source, ProviderCacheKind::DeepSeekPromptCache);
}

#[test]
fn no_cache_fields_returns_none() {
    let raw = serde_json::json!({
        "prompt_tokens": 10,
        "completion_tokens": 5,
        "total_tokens": 15
    });

    let p = test_provider();
    let resp = llm_proxy::types::ChatCompletionResponse {
        id: "3".into(),
        object: "chat.completion".into(),
        created: 2,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
        raw_usage_json: Some(raw),
    };

    let audit = p.extract_audit(&ProviderResponse::Once(resp));
    assert_eq!(audit.cache.source, ProviderCacheKind::None);
    assert_eq!(audit.cache.hit_tokens, None);
}

#[test]
fn missing_raw_usage_returns_default() {
    let p = test_provider();
    let resp = llm_proxy::types::ChatCompletionResponse {
        id: "4".into(),
        object: "chat.completion".into(),
        created: 3,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
        raw_usage_json: None,
    };

    let audit = p.extract_audit(&ProviderResponse::Once(resp));
    assert_eq!(audit.cache.source, ProviderCacheKind::None);
}

#[test]
fn stream_returns_default_audit() {
    let p = test_provider();
    let stream = futures::stream::empty();
    let resp = ProviderResponse::Stream {
        body: Box::pin(stream),
    };
    let audit = p.extract_audit(&resp);
    assert_eq!(audit.cache.source, ProviderCacheKind::None);
}

#[test]
fn reasoning_tokens_are_extracted() {
    let raw = serde_json::json!({
        "completion_tokens_details": {
            "reasoning_tokens": 200,
            "audio_tokens": 50
        }
    });

    let p = test_provider();
    let resp = llm_proxy::types::ChatCompletionResponse {
        id: "5".into(),
        object: "chat.completion".into(),
        created: 4,
        model: "o1-mini".into(),
        choices: vec![],
        usage: None,
        raw_usage_json: Some(raw),
    };

    let audit = p.extract_audit(&ProviderResponse::Once(resp));
    assert_eq!(audit.reasoning_tokens, Some(200));
    assert_eq!(audit.audio_tokens, Some(50));
}

#[test]
fn default_provider_extract_audit_returns_empty() {
    // The default impl on the trait should return AuditFromProvider::default()
    struct DefaultProvider;
    #[async_trait::async_trait]
    impl Provider for DefaultProvider {
        fn id(&self) -> &str {
            "default"
        }
        fn base_url(&self) -> &str {
            "https://x"
        }
        async fn chat(
            &self,
            _req: &ChatCompletionRequest,
            _key: &KeyEntry,
        ) -> Result<ProviderResponse, llm_proxy::error::AppError> {
            unimplemented!()
        }
    }

    let p = DefaultProvider;
    let resp = ProviderResponse::Once(llm_proxy::types::ChatCompletionResponse {
        id: "x".into(),
        object: "x".into(),
        created: 0,
        model: "x".into(),
        choices: vec![],
        usage: None,
        raw_usage_json: None,
    });
    let audit = p.extract_audit(&resp);
    assert_eq!(audit.cache.source, ProviderCacheKind::None);
}

// ── SQLite round-trip tests ─────────────────────────────────────────────

#[tokio::test]
async fn log_with_openai_cache_writes_columns() {
    let (pool, _dir) = setup_db().await;

    let audit = AuditDetail {
        from_provider: AuditFromProvider {
            cache: CacheReport {
                hit_tokens: Some(800),
                creation_tokens: None,
                source: ProviderCacheKind::OpenAiPromptCache,
            },
            ..Default::default()
        },
        from_router: AuditFromRouter::default(),
        from_auth: AuditFromAuth::default(),
    };

    let log = RequestLog {
        id: uuid::Uuid::new_v4().to_string(),
        ts: 1,
        client_ip: None,
        model: "gpt-4o".into(),
        pool_id: "p1".into(),
        key_hash: db::compute_key_hash("k"),
        upstream: None,
        status_code: Some(200),
        latency_ms: Some(10),
        prompt_tokens: Some(100),
        completion_tokens: Some(50),
        total_tokens: Some(150),
        is_stream: false,
        error: None,
        audit,
        error_code: None,
        user_agent: None,
        cost_usd: None,
    };

    db::log_request(&pool, &log).await.unwrap();

    let row = sqlx::query("SELECT * FROM request_log WHERE id = ?1")
        .bind(&log.id)
        .fetch_one(&pool)
        .await
        .unwrap();

    let cached: Option<i32> = row.get("cached_tokens");
    assert_eq!(cached, Some(800));
    let source: Option<String> = row.get("cache_source");
    assert_eq!(source, Some("OpenAiPromptCache".into()));
}

#[tokio::test]
async fn log_without_cache_writes_nulls() {
    let (pool, _dir) = setup_db().await;

    let log = RequestLog {
        id: uuid::Uuid::new_v4().to_string(),
        ts: 2,
        client_ip: None,
        model: "m".into(),
        pool_id: "p".into(),
        key_hash: db::compute_key_hash("k"),
        upstream: None,
        status_code: Some(200),
        latency_ms: Some(10),
        prompt_tokens: Some(10),
        completion_tokens: Some(5),
        total_tokens: Some(15),
        is_stream: false,
        error: None,
        audit: AuditDetail::none(),
        error_code: None,
        user_agent: None,
        cost_usd: None,
    };

    db::log_request(&pool, &log).await.unwrap();

    let row = sqlx::query("SELECT * FROM request_log WHERE id = ?1")
        .bind(&log.id)
        .fetch_one(&pool)
        .await
        .unwrap();

    let cached: Option<i32> = row.get("cached_tokens");
    assert!(cached.is_none());
    let source: Option<String> = row.get("cache_source");
    assert!(source.is_none());
}

#[tokio::test]
async fn log_with_retry_count_tenant_id() {
    let (pool, _dir) = setup_db().await;

    let log = RequestLog {
        id: uuid::Uuid::new_v4().to_string(),
        ts: 3,
        client_ip: None,
        model: "m".into(),
        pool_id: "p".into(),
        key_hash: db::compute_key_hash("k"),
        upstream: None,
        status_code: Some(200),
        latency_ms: Some(5),
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        is_stream: true,
        error: None,
        audit: AuditDetail {
            from_router: AuditFromRouter {
                retry_count: 3,
                ttft_ms: Some(250),
            },
            from_auth: AuditFromAuth {
                tenant_id: "org-42".into(),
            },
            ..AuditDetail::none()
        },
        error_code: None,
        user_agent: None,
        cost_usd: None,
    };

    db::log_request(&pool, &log).await.unwrap();

    let row = sqlx::query("SELECT * FROM request_log WHERE id = ?1")
        .bind(&log.id)
        .fetch_one(&pool)
        .await
        .unwrap();

    let retry: i32 = row.get("retry_count");
    assert_eq!(retry, 3);
    let ttft: Option<i32> = row.get("ttft_ms");
    assert_eq!(ttft, Some(250));
    let tenant: String = row.get("tenant_id");
    assert_eq!(tenant, "org-42");
}

#[tokio::test]
async fn error_code_is_pool_exhausted() {
    let (pool, _dir) = setup_db().await;
    let err = llm_proxy::error::AppError::Internal("all keys in pool 'p1' exhausted".into());
    let code = ErrorCode::from_app_error(&err);
    assert_eq!(code.to_string(), "PoolExhausted");

    let log = RequestLog {
        id: uuid::Uuid::new_v4().to_string(),
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
        error: Some("all keys in pool 'p1' exhausted".into()),
        audit: AuditDetail::none(),
        error_code: Some(code.to_string()),
        user_agent: None,
        cost_usd: None,
    };

    db::log_request(&pool, &log).await.unwrap();

    let row = sqlx::query("SELECT * FROM request_log WHERE id = ?1")
        .bind(&log.id)
        .fetch_one(&pool)
        .await
        .unwrap();

    let ec: Option<String> = row.get("error_code");
    assert_eq!(ec, Some("PoolExhausted".into()));
}

#[test]
fn finish_reason_and_upstream_model_are_extracted() {
    let p = test_provider();
    let resp = llm_proxy::types::ChatCompletionResponse {
        id: "6".into(),
        object: "chat.completion".into(),
        created: 5,
        model: "gpt-4o-2024-08-06".into(),
        choices: vec![llm_proxy::types::Choice {
            index: 0,
            message: llm_proxy::types::ResponseMessage {
                role: "assistant".into(),
                content: Some("ok".into()),
                tool_calls: None,
                reasoning_content: None,
                reasoning: None,
                reasoning_text: None,
                thinking: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        raw_usage_json: None,
    };

    let audit = p.extract_audit(&ProviderResponse::Once(resp));
    assert_eq!(audit.finish_reason, Some("stop".into()));
    assert_eq!(audit.upstream_model, Some("gpt-4o-2024-08-06".into()));
}

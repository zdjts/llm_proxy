//! End-to-end tests for per-model thinking-level translation.
//!
//! Covers the request path wired in `chat_completions_handler`:
//! `reasoning_effort` from the client is translated through the
//! `model_registry` catalog (`capabilities_json.metadata.thinkingLevelMap`)
//! before the request reaches the upstream, and unsupported canonical levels
//! are rejected with 400.
//!
//! All upstream HTTP calls are intercepted by `mockito`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use llm_proxy::auth::{AuthState, ClientKeyEntry};
use llm_proxy::config::{KeyEntry, PoolConfig};
use llm_proxy::router::{BadKeyRegistry, Router, RouterHandle};
use llm_proxy::server::{self, AppState};
use mockito::Matcher;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

async fn test_state(db: sqlx::SqlitePool, upstream_url: &str) -> AppState {
    let config =
        llm_proxy::config::Config::load(std::path::Path::new("config.example.yaml")).unwrap();

    let mut pools = HashMap::new();
    pools.insert(
        "pool-a".to_owned(),
        PoolConfig {
            keys: vec![KeyEntry::api_key("upstream-test-key", 1)],
        },
    );
    // providers must exist in the ConfigStore snapshot for the router to
    // have been built from it in production; here the Router is built
    // directly, so only the pool wiring is needed.
    let model_map = std::iter::once((
        "grok-4.6".to_owned(),
        ("pool-a".to_owned(), pools["pool-a"].clone(), None, None),
    ))
    .collect();
    let router = RouterHandle::new(Arc::new(Router::new(
        model_map,
        std::iter::once((
            "pool-a".to_owned(),
            Arc::new(llm_proxy::provider::openai::OpenAiProvider::new(
                "prov-a".into(),
                upstream_url.to_owned(),
                Arc::from([401u16, 402, 403, 429]),
            )) as Arc<dyn llm_proxy::provider::Provider>,
        ))
        .collect(),
        Arc::new(BadKeyRegistry::new()),
    )));
    let model_metadata = config.model_metadata.clone();
    let snapshot = Arc::new(config);
    let (alert_tx, _) = tokio::sync::broadcast::channel(4);
    let config_store = Arc::new(llm_proxy::config_store::ConfigStore::for_test(
        db.clone(),
        model_metadata,
    ));
    AppState {
        router: router.clone(),
        catalog: llm_proxy::model_catalog::ModelCatalog::new(router, Arc::clone(&config_store)),
        db,
        config: Arc::clone(&snapshot),
        cache: llm_proxy::cache::PromptCache::new(0),
        metrics: Arc::new(llm_proxy::metrics::Metrics::default()),
        circuit_breaker: Arc::new(llm_proxy::circuit_breaker::CircuitBreaker::with_defaults()),
        concurrency: Arc::new(llm_proxy::concurrency::ConcurrencyLimiter::new(10, 10)),
        alert_tx,
        error_burst_counters: Arc::new(dashmap::DashMap::new()),
        alert_snapshot: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        auth_store: None,
        config_store,
        credentials: Arc::new(llm_proxy::credential::CredentialRuntime::new(None)),
    }
}

/// Seed a DB-backed model_registry entry for `grok-4.6` with an xAI-style
/// legacy `none`-keyed thinkingLevelMap (as written by older imports).
async fn seed_registry(db: &sqlx::SqlitePool, map: serde_json::Value) {
    let capabilities = serde_json::json!({
        "reasoning": true,
        "metadata": { "thinkingLevelMap": map }
    });
    sqlx::query(
        "INSERT INTO model_registry (
            id, display_name, provider_kind, provider_config_id,
            supports_vision, supports_tool_calling, supports_json_mode,
            max_context_tokens, max_output_tokens,
            input_price_per_1m, output_price_per_1m,
            capabilities_json, enabled
        ) VALUES ('grok-4.6','Grok 4.6','openai',NULL,1,1,0,500000,30000,2.0,6.0,?1,1)",
    )
    .bind(capabilities.to_string())
    .execute(db)
    .await
    .unwrap();
}

async fn setup_db() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new()
        .connect_lazy("sqlite::memory:")
        .unwrap();
    let migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
        .await
        .unwrap();
    migrator.run(&pool).await.unwrap();
    pool
}

fn app(state: AppState) -> axum::Router {
    server::build_router(
        state,
        AuthState {
            store: None,
            entries: vec![ClientKeyEntry {
                key: "client-test-key".to_owned(),
                tenant_id: "default".to_owned(),
            }],
        },
        None,
    )
}

fn chat_request(extra: serde_json::Value) -> Body {
    let mut body = serde_json::json!({
        "model": "grok-4.6",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 16
    });
    if let (Some(obj), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    Body::from(body.to_string())
}

fn upstream_body() -> String {
    r#"{"id":"u1","object":"chat.completion","created":1,"model":"grok-4.6","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#.into()
}

#[tokio::test]
async fn canonical_off_is_translated_to_upstream_none() {
    let mut upstream = mockito::Server::new_async().await;
    // The upstream must see the wire value `none`, not pi's `off`.
    let mock = upstream
        .mock("POST", "/chat/completions")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "reasoning_effort": "none"
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(upstream_body())
        .create_async()
        .await;

    let db = setup_db().await;
    seed_registry(
        &db,
        serde_json::json!({"none": "none", "low": "low", "high": "high"}),
    )
    .await;
    let state = test_state(db, &upstream.url()).await;
    // The translation reads the DB-backed registry: refresh the store first.
    state.config_store.refresh_from_db().await.unwrap();

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header(header::AUTHORIZATION, "Bearer client-test-key")
                .header(header::CONTENT_TYPE, "application/json")
                .body(chat_request(serde_json::json!({"reasoning_effort": "off"})))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn unsupported_canonical_level_is_rejected_with_400() {
    let upstream = mockito::Server::new_async().await;
    let db = setup_db().await;
    // hy4-style catalog: only `high` (and disabled) exist.
    seed_registry(&db, serde_json::json!({"high": "high", "none": "none"})).await;
    let state = test_state(db, &upstream.url()).await;
    state.config_store.refresh_from_db().await.unwrap();

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header(header::AUTHORIZATION, "Bearer client-test-key")
                .header(header::CONTENT_TYPE, "application/json")
                .body(chat_request(serde_json::json!({"reasoning_effort": "low"})))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not support reasoning_effort='low'"),
        "unexpected error body: {json}"
    );
}

#[tokio::test]
async fn canonicalised_map_is_advertised_on_model_metadata() {
    let upstream = mockito::Server::new_async().await;
    let db = setup_db().await;
    seed_registry(
        &db,
        serde_json::json!({"none": "none", "low": "low", "high": "high"}),
    )
    .await;
    let state = test_state(db, &upstream.url()).await;
    state.config_store.refresh_from_db().await.unwrap();

    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/v1/model-metadata")
                .header(header::AUTHORIZATION, "Bearer client-test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let entry = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "grok-4.6")
        .unwrap();
    let map = entry["thinking_level_map"].as_object().unwrap();
    assert_eq!(
        map["off"], "none",
        "legacy `none` key canonicalised to `off`"
    );
    assert_eq!(map["low"], "low");
    assert_eq!(map["high"], "high");
    assert!(map.get("none").is_none());
    // The flat level list agrees with the map.
    let levels = entry["thinking_levels"].as_array().unwrap();
    assert_eq!(
        levels,
        &serde_json::json!(["off", "low", "high"])
            .as_array()
            .unwrap()
            .clone(),
        "levels derived from the canonicalised map"
    );
}

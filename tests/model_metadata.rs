use std::collections::HashMap;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use llm_proxy::auth::{AuthState, ClientKeyEntry};
use llm_proxy::config::{KeyEntry, PoolConfig, PoolStrategy};
use llm_proxy::router::{BadKeyRegistry, Router, RouterHandle};
use llm_proxy::server::{self, AppState};
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

fn test_state(models: &[(&str, &str)]) -> AppState {
    let mut config =
        llm_proxy::config::Config::load(std::path::Path::new("config.example.yaml")).unwrap();
    config.model_metadata = serde_yaml::from_str(
        r#"
defaults:
  context_window: 100
  pricing:
    input_usd_per_million_tokens: 1.0
pools:
  pool-a:
    supports_tools: true
models:
  z-model:
    name: "Zed"
    max_output_tokens: 42
"#,
    )
    .unwrap();

    let mut pools = HashMap::new();
    pools.insert(
        "pool-a".to_owned(),
        PoolConfig {
            keys: vec![KeyEntry {
                key: "upstream-test-key".to_owned(),
                weight: 1,
            }],
            strategy: PoolStrategy::WeightedRandom,
        },
    );
    let model_map = models
        .iter()
        .map(|(model, pool)| {
            (
                (*model).to_owned(),
                ((*pool).to_owned(), pools[*pool].clone(), None),
            )
        })
        .collect();
    let router = RouterHandle::new(Arc::new(Router::new(
        model_map,
        HashMap::new(),
        Arc::new(BadKeyRegistry::new()),
    )));
    let db = SqlitePoolOptions::new()
        .connect_lazy("sqlite::memory:")
        .unwrap();
    let config_store = Arc::new(llm_proxy::config_store::ConfigStore::for_test(
        db.clone(),
        config.model_metadata.clone(),
    ));
    let snapshot = Arc::new(config);
    let (alert_tx, _) = tokio::sync::broadcast::channel(4);
    AppState {
        router: router.clone(),
        catalog: llm_proxy::model_catalog::ModelCatalog::new(router, Arc::clone(&config_store)),
        db,
        config: Arc::clone(&snapshot),
        cache: llm_proxy::cache::PromptCache::new(0),
        metrics: Arc::new(llm_proxy::metrics::Metrics::default()),
        circuit_breaker: Arc::new(llm_proxy::circuit_breaker::CircuitBreaker::with_defaults()),
        concurrency: Arc::new(llm_proxy::concurrency::ConcurrencyLimiter::new(10, 10)),
        fallback_config: Arc::new(llm_proxy::fallback::FallbackConfig::default()),
        alert_tx,
        error_burst_counters: Arc::new(dashmap::DashMap::new()),
        alert_snapshot: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        auth_store: None,
        quota_tracker: None,
        pipeline: None,
        rbac_state: None,
        config_store,
        budget_manager: None,
    }
}

fn app(models: &[(&str, &str)]) -> axum::Router {
    server::build_router(
        test_state(models),
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

#[tokio::test]
async fn metadata_requires_authentication() {
    let response = app(&[("m", "pool-a")])
        .oneshot(
            Request::builder()
                .uri("/v1/model-metadata")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn metadata_returns_sorted_live_models_and_merged_json() {
    let response = app(&[("z-model", "pool-a"), ("a-model", "pool-a")])
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
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["object"], "model_metadata_list");
    assert_eq!(json["data"][0]["id"], "a-model");
    assert_eq!(json["data"][1]["id"], "z-model");
    assert_eq!(json["data"][1]["name"], "Zed");
    assert_eq!(json["data"][1]["context_window"], 100);
    assert_eq!(json["data"][1]["max_output_tokens"], 42);
    assert_eq!(json["data"][1]["supports_tools"], true);
    assert_eq!(
        json["data"][1]["pricing"]["input_usd_per_million_tokens"],
        1.0
    );
}

#[tokio::test]
async fn models_and_metadata_expose_same_live_sorted_ids() {
    let models_response = app(&[("z-model", "pool-a"), ("a-model", "pool-a")])
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer client-test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let metadata_response = app(&[("z-model", "pool-a"), ("a-model", "pool-a")])
        .oneshot(
            Request::builder()
                .uri("/v1/model-metadata")
                .header(header::AUTHORIZATION, "Bearer client-test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(models_response.status(), StatusCode::OK);
    assert_eq!(metadata_response.status(), StatusCode::OK);
    let models: serde_json::Value = serde_json::from_slice(
        &to_bytes(models_response.into_body(), 16 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(
        &to_bytes(metadata_response.into_body(), 16 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let model_ids: Vec<_> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["id"].clone())
        .collect();
    let metadata_ids: Vec<_> = metadata["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["id"].clone())
        .collect();
    assert_eq!(model_ids, metadata_ids);
    assert_eq!(
        model_ids,
        vec![serde_json::json!("a-model"), serde_json::json!("z-model")]
    );
}
#[tokio::test]
async fn metadata_returns_empty_data_for_empty_live_router() {
    let response = app(&[])
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
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"object":"model_metadata_list","data":[]})
    );
}

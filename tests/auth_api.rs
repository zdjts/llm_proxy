//! Integration tests for IP-guarded admin config and client-key APIs.

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
    let db_path = dir.path().join("test_auth.db");
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

#[tokio::test]
async fn admin_ip_guard_hides_config_routes_from_non_whitelisted_ip() {
    let (_pool, _dir, state) = setup().await;
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/refresh")
                .header("x-real-ip", "192.0.2.10")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn config_validate_accepts_example_yaml() {
    let (_pool, _dir, state) = setup().await;
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/validate")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"yaml": include_str!("fixtures/config.yaml")}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn config_export_returns_importable_yaml() {
    let (_pool, _dir, state) = setup().await;
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/config/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let yaml = String::from_utf8(body.to_vec()).unwrap();
    assert!(yaml.contains("server:\n  host: 127.0.0.1\n  port: 4000"));
    assert!(yaml.contains("max_body_bytes:"));
    assert!(yaml.contains("db:\n  path: ./test.db"));
    assert!(
        !yaml.contains("kind: open_ai"),
        "export must use openai not open_ai"
    );
    llm_proxy::config_store::ConfigStore::validate_yaml(&yaml).unwrap();
}

#[tokio::test]
async fn config_export_preserves_response_normalization_round_trip() {
    use llm_proxy::config::ResponseNormalization;

    // Build a state whose startup policy has a non-default
    // `response_normalization`.  The export document must include that
    // flag so an operator who exports, edits another section, and
    // re-imports does not silently lose it.
    let (_pool, _dir, state) = setup().await;
    let custom = ResponseNormalization {
        strip_think_tags: true,
    };
    let runtime = std::sync::Arc::new(llm_proxy::config_store::RuntimePolicy {
        failover: state.config.failover.clone(),
        alerts: state.config.alerts.clone(),
        cache_max_entries: state.config.cache_max_entries,
        response_normalization: custom.clone(),
    });
    state
        .config_store
        .set_bootstrap_runtime((*runtime).clone())
        .await;

    // Export.
    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/config/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let yaml = String::from_utf8(body.to_vec()).unwrap();

    // Assert the YAML contains the operator-chosen settings.
    assert!(
        yaml.contains("strip_think_tags: true"),
        "export must include strip_think_tags: {yaml}"
    );
    assert!(
        !yaml.contains("think_tag_pairs:"),
        "export must not include think_tag_pairs: {yaml}"
    );

    let parsed = llm_proxy::config_store::ConfigStore::validate_yaml(&yaml).unwrap();
    assert_eq!(
        parsed.response_normalization.strip_think_tags,
        custom.strip_think_tags
    );
}

#[tokio::test]
async fn config_export_can_be_imported_without_losing_provider_metadata() {
    let (pool, _dir, state) = setup().await;
    sqlx::query("INSERT INTO key_pool (id, strategy) VALUES ('export-pool', 'weighted_random')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO key_entry (pool_id, key_hash, key_plain, weight) VALUES (?1, ?2, ?3, 2)",
    )
    .bind("export-pool")
    .bind(llm_proxy::db::compute_key_hash("sk-export"))
    .bind("sk-export")
    .execute(&pool)
    .await
    .unwrap();
    let metadata = serde_json::json!({
        "api_key": "provider-secret",
        "nested": { "token": "nested-secret", "retries": 3 },
        "headers": ["x-request-id", "x-trace-id"]
    });
    sqlx::query(
        "INSERT INTO provider_config (id, kind, base_url, pool_id, metadata) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind("export-provider")
    .bind("openai")
    .bind("https://provider.example/v1")
    .bind("export-pool")
    .bind(serde_json::to_string(&metadata).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO routing_config (logical_model, pool_id, default_params) VALUES (?1, ?2, ?3)",
    )
    .bind("export-model")
    .bind("export-pool")
    .bind(r#"{"temperature":0.2}"#)
    .execute(&pool)
    .await
    .unwrap();
    state.config_store.refresh_from_db().await.unwrap();

    let router = app(state);
    let export = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/config/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(export.status(), StatusCode::OK);
    let yaml = String::from_utf8(
        axum::body::to_bytes(export.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();

    let import = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/import")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "yaml": yaml }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let import_status = import.status();
    let import_body = axum::body::to_bytes(import.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert_eq!(
        import_status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&import_body)
    );

    let restored: String =
        sqlx::query_scalar("SELECT metadata FROM provider_config WHERE id = 'export-provider'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&restored).unwrap(),
        metadata
    );
    let restored_key: (String, i64) =
        sqlx::query_as("SELECT key_plain, weight FROM key_entry WHERE pool_id = 'export-pool'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(restored_key, ("sk-export".into(), 2));
    let restored_strategy: String =
        sqlx::query_scalar("SELECT strategy FROM key_pool WHERE id = 'export-pool'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(restored_strategy, "weighted_random");
    let restored_params: String = sqlx::query_scalar(
        "SELECT default_params FROM routing_config WHERE logical_model = 'export-model'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&restored_params).unwrap(),
        serde_json::json!({ "temperature": 0.2 })
    );
}

#[tokio::test]
async fn config_export_round_trips_model_registry_metadata() {
    let (pool, _dir, state) = setup().await;
    sqlx::query("INSERT INTO key_pool (id, strategy) VALUES ('registry-pool', 'weighted_random')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO key_entry (pool_id, key_hash, key_plain, weight) VALUES (?1, ?2, ?3, 1)",
    )
    .bind("registry-pool")
    .bind(llm_proxy::db::compute_key_hash("sk-registry"))
    .bind("sk-registry")
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO provider_config (id, kind, base_url, pool_id, metadata) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind("registry-provider")
    .bind("openai")
    .bind("https://registry.example/v1")
    .bind("registry-pool")
    .bind("{}")
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO routing_config (logical_model, pool_id, default_params) VALUES (?1, ?2, ?3)",
    )
    .bind("grok-4.6")
    .bind("registry-pool")
    .bind(r#"{"temperature":0.1}"#)
    .execute(&pool)
    .await
    .unwrap();
    let capabilities = serde_json::json!({
        "reasoning": true,
        "metadata": {"thinkingLevelMap": {"low": "low", "high": "high"}}
    });
    sqlx::query(
        "INSERT INTO model_registry (
            id, display_name, provider_kind, provider_config_id,
            supports_vision, supports_tool_calling, supports_json_mode,
            max_context_tokens, max_output_tokens,
            input_price_per_1m, output_price_per_1m,
            capabilities_json, enabled
        ) VALUES (?1,?2,?3,?4,1,1,0,128000,64000,2.0,6.0,?5,1)",
    )
    .bind("grok-4.6")
    .bind("Grok 4.6")
    .bind("openai")
    .bind("registry-provider")
    .bind(capabilities.to_string())
    .execute(&pool)
    .await
    .unwrap();
    state.config_store.refresh_from_db().await.unwrap();

    let router = app(state);
    let export = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/config/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(export.status(), StatusCode::OK);
    let yaml = String::from_utf8(
        axum::body::to_bytes(export.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(yaml.contains("grok-4.6"));
    assert!(
        !yaml.contains("model_registry:"),
        "config export must omit models.dev catalog rows"
    );

    sqlx::query("DELETE FROM model_registry WHERE id = 'grok-4.6'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM routing_config WHERE logical_model = 'grok-4.6'")
        .execute(&pool)
        .await
        .unwrap();

    let import = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/import")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "yaml": yaml }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::OK);

    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM model_registry WHERE id = 'grok-4.6'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0);
    let route_pool: String =
        sqlx::query_scalar("SELECT pool_id FROM routing_config WHERE logical_model = 'grok-4.6'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(route_pool, "registry-pool");
}

#[tokio::test]
async fn creating_model_with_provider_auto_creates_routing() {
    let (pool, _dir, state) = setup().await;
    sqlx::query("INSERT INTO key_pool (id, strategy) VALUES ('auto-pool', 'weighted_random')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO key_entry (pool_id, key_hash, key_plain, weight) VALUES (?1, ?2, ?3, 1)",
    )
    .bind("auto-pool")
    .bind(llm_proxy::db::compute_key_hash("sk-auto"))
    .bind("sk-auto")
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO provider_config (id, kind, base_url, pool_id, metadata) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind("auto-provider")
    .bind("openai")
    .bind("https://auto.example/v1")
    .bind("auto-pool")
    .bind("{}")
    .execute(&pool)
    .await
    .unwrap();
    state.config_store.refresh_from_db().await.unwrap();

    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/models")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "id": "grok-4.6",
                        "display_name": "Grok 4.6",
                        "provider_kind": "openai",
                        "provider_config_id": "auto-provider",
                        "supports_vision": true,
                        "supports_tool_calling": true,
                        "supports_json_mode": false,
                        "max_context_tokens": 128000,
                        "max_output_tokens": 64000,
                        "input_price_per_1m": 2.0,
                        "output_price_per_1m": 6.0,
                        "capabilities_json": {"reasoning": true},
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let route_pool: String =
        sqlx::query_scalar("SELECT pool_id FROM routing_config WHERE logical_model = 'grok-4.6'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(route_pool, "auto-pool");
    assert!(
        state
            .config_store
            .snapshot()
            .await
            .model_routing
            .contains_key("grok-4.6")
    );
}

#[tokio::test]
async fn it_refreshes_db_config_via_admin_endpoint() {
    let (_pool, _dir, state) = setup().await;
    let resp = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/refresh")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["version"].as_u64().unwrap() > 0);
}

use llm_proxy::db;
use llm_proxy::model_import;
use sqlx::SqlitePool;
use tempfile::TempDir;

async fn setup() -> (SqlitePool, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("import.db");
    (db::connect(path.to_str().unwrap()).await.unwrap(), dir)
}

async fn seed(pool: &SqlitePool) {
    sqlx::query("INSERT INTO key_pool (id, enabled) VALUES ('pool-a', 1)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO provider_config (id, kind, base_url, pool_id, enabled) VALUES ('provider-a', 'openai', 'http://localhost', 'pool-a', 1)")
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO routing_config (logical_model, pool_id, enabled) VALUES ('grok-4.5_oa', 'pool-a', 1)")
        .execute(pool).await.unwrap();
}

fn source(extra: &str) -> String {
    format!(
        r#"{{"xai":{{"models":{{"grok-4.5":{{"id":"grok-4.5","name":"Grok 4.5","limit":{{"context":500000,"output":500000}},"modalities":{{"input":["text","image"]}},"reasoning":true,"reasoning_options":[{{"type":"effort","values":["low","medium","high"]}}],"tool_call":true,"structured_output":true,"cost":{{"input":2.0,"output":6.0,"cache_read":0.3}},"apiKey":"do-not-store"{extra}}}}}}}}}"#
    )
}

#[tokio::test]
async fn imports_matching_registry_row_and_is_visible_after_refresh() {
    let (pool, _dir) = setup().await;
    seed(&pool).await;
    let report = model_import::import_json(&pool, &source(""), false)
        .await
        .unwrap();
    assert_eq!(report.counts.new, 1);
    let row: (String, i64, i64, f64, f64, String) = sqlx::query_as("SELECT display_name, supports_vision, supports_tool_calling, input_price_per_1m, output_price_per_1m, capabilities_json FROM model_registry WHERE id='grok-4.5_oa'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(row.0, "Grok 4.5");
    assert_eq!(row.1, 1);
    assert_eq!(row.2, 1);
    assert_eq!(row.3, 2.0);
    assert_eq!(row.4, 6.0);
    assert!(!row.5.contains("do-not-store"));
    assert!(!row.5.contains("apiKey"));
    assert!(row.5.contains("thinkingLevelMap"));
}

#[tokio::test]
async fn default_import_is_idempotent_and_protects_admin_row() {
    let (pool, _dir) = setup().await;
    seed(&pool).await;
    model_import::import_json(&pool, &source(""), false)
        .await
        .unwrap();
    let second = model_import::import_json(&pool, &source(""), false)
        .await
        .unwrap();
    assert_eq!(second.counts.new, 0);
    assert_eq!(second.counts.conflicts, 1);
    sqlx::query("UPDATE model_registry SET display_name='Admin Name' WHERE id='grok-4.5_oa'")
        .execute(&pool)
        .await
        .unwrap();
    model_import::import_json(&pool, &source(""), false)
        .await
        .unwrap();
    let name: String =
        sqlx::query_scalar("SELECT display_name FROM model_registry WHERE id='grok-4.5_oa'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(name, "Admin Name");
}

#[tokio::test]
async fn malformed_item_fails_without_partial_write() {
    let (pool, _dir) = setup().await;
    seed(&pool).await;
    let bad = source(r#","cost":{"input":-1.0,"output":6.0}"#);
    assert!(model_import::import_json(&pool, &bad, false).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM model_registry")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn missing_association_is_skipped_without_importing_catalog_models() {
    let (pool, _dir) = setup().await;
    let report = model_import::import_json(&pool, &source(""), false)
        .await
        .unwrap();
    assert_eq!(report.counts.new, 0);
    assert_eq!(report.counts.skipped, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM model_registry")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn duplicate_source_ids_are_rejected_within_a_provider() {
    let (pool, _dir) = setup().await;
    let json = r#"{"xai":{"models":{"a":{"id":"same"},"b":{"id":"same"}}}}"#;
    let error = model_import::import_json(&pool, json, false)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("duplicate source model id"));
}

#[tokio::test]
async fn same_model_id_across_providers_is_allowed() {
    let (pool, _dir) = setup().await;
    let json =
        r#"{"xai":{"models":{"same":{"id":"same"}}},"openai":{"models":{"same":{"id":"same"}}}}"#;
    let report = model_import::import_json(&pool, json, false).await.unwrap();
    assert_eq!(report.counts.new, 0);
}

#[tokio::test]
async fn explicit_overwrite_updates_existing_record() {
    let (pool, _dir) = setup().await;
    seed(&pool).await;
    model_import::import_json(&pool, &source(""), false)
        .await
        .unwrap();
    sqlx::query("UPDATE model_registry SET display_name='Admin Name'")
        .execute(&pool)
        .await
        .unwrap();
    let report = model_import::import_json(&pool, &source(""), true)
        .await
        .unwrap();
    assert_eq!(report.counts.new, 1);
    let name: String = sqlx::query_scalar("SELECT display_name FROM model_registry")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "Grok 4.5");
}

#[tokio::test]
async fn disabled_pool_or_provider_is_skipped() {
    let (pool, _dir) = setup().await;
    sqlx::query("INSERT INTO key_pool (id, enabled) VALUES ('pool-a', 0)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO routing_config (logical_model, pool_id, enabled) VALUES ('grok-4.5_oa', 'pool-a', 1)")
        .execute(&pool).await.unwrap();
    let report = model_import::import_json(&pool, &source(""), false)
        .await
        .unwrap();
    assert_eq!(report.counts.skipped, 1);
    assert!(
        report
            .reasons
            .iter()
            .all(|reason| !reason.contains("do-not-store"))
    );
}

#[tokio::test]
async fn imported_registry_is_visible_through_catalog_after_refresh() {
    let (pool, _dir) = setup().await;
    seed(&pool).await;
    model_import::import_json(&pool, &source(""), false)
        .await
        .unwrap();
    let store = std::sync::Arc::new(
        llm_proxy::config_store::ConfigStore::load(pool.clone())
            .await
            .unwrap(),
    );
    let mut pools = std::collections::HashMap::new();
    pools.insert(
        "grok-4.5_oa".to_owned(),
        (
            "pool-a".to_owned(),
            llm_proxy::config::PoolConfig {
                keys: vec![llm_proxy::config::KeyEntry::api_key("test", 1)],
            },
            None,
            None,
        ),
    );
    let router =
        llm_proxy::router::RouterHandle::new(std::sync::Arc::new(llm_proxy::router::Router::new(
            pools,
            std::collections::HashMap::new(),
            std::sync::Arc::new(llm_proxy::router::BadKeyRegistry::new()),
        )));
    let metadata = llm_proxy::model_catalog::ModelCatalog::new(router, store)
        .list_metadata()
        .await;
    assert_eq!(metadata[0].name, "Grok 4.5");
    assert_eq!(metadata[0].context_window, 500000);
    assert_eq!(metadata[0].pricing.input_usd_per_million_tokens, 2.0);
}

#[tokio::test]
async fn import_source_fetches_models_dev_catalog_over_http() {
    let (pool, _dir) = setup().await;
    seed(&pool).await;
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/api.json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(source(""))
        .create_async()
        .await;
    let url = format!("{}/api.json", server.url());
    let report = model_import::import_source(&pool, &url, false)
        .await
        .unwrap();
    mock.assert_async().await;
    assert_eq!(report.counts.new, 1);
}

fn glm_catalog() -> String {
    r#"{
      "bothub": {"models": {"glm-5.3-flash": {
        "id": "glm-5.3-flash", "name": "GLM-5.3-Flash",
        "limit": {"context": 1000000, "output": 131072},
        "modalities": {"input": ["text"]},
        "tool_call": true, "structured_output": true,
        "cost": {"input": 0.12, "output": 0.44}
      }}},
      "zai": {"models": {"glm-5.3-flash": {
        "id": "glm-5.3-flash", "name": "GLM-5.3-Flash",
        "limit": {"context": 1000000, "output": 131072},
        "modalities": {"input": ["text"]},
        "tool_call": true, "structured_output": true,
        "cost": {"input": 0.075, "output": 0.25}
      }}}
    }"#
    .to_owned()
}

#[tokio::test]
async fn imports_official_catalog_model_onto_openai_compatible_pool() {
    let (pool, _dir) = setup().await;
    sqlx::query("INSERT INTO key_pool (id, enabled) VALUES ('pool-a', 1)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO provider_config (id, kind, base_url, pool_id, enabled) VALUES ('x5m5x', 'openai', 'http://localhost', 'pool-a', 1)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO routing_config (logical_model, pool_id, enabled) VALUES ('glm-5.3-flash', 'pool-a', 1)")
        .execute(&pool).await.unwrap();
    let report = model_import::import_json(&pool, &glm_catalog(), false)
        .await
        .unwrap();
    assert_eq!(report.counts.new, 1, "{report:?}");
    let row: (String, f64, f64) = sqlx::query_as(
        "SELECT display_name, input_price_per_1m, output_price_per_1m FROM model_registry WHERE id='glm-5.3-flash'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "GLM-5.3-Flash");
    assert_eq!(row.1, 0.075);
    assert_eq!(row.2, 0.25);
}

#[tokio::test]
async fn imports_catalog_using_upstream_model_alias() {
    let (pool, _dir) = setup().await;
    sqlx::query("INSERT INTO key_pool (id, enabled) VALUES ('pool-a', 1)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO provider_config (id, kind, base_url, pool_id, enabled) VALUES ('deepseek', 'openai', 'http://localhost', 'pool-a', 1)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO routing_config (logical_model, pool_id, upstream_model, enabled) VALUES ('deepseek-v4-flash', 'pool-a', 'deepseek-v4-flash-0731', 1)")
        .execute(&pool).await.unwrap();
    let json = r#"{"deepseek":{"models":{"deepseek-v4-flash-0731":{"id":"deepseek-v4-flash-0731","name":"DeepSeek V4 Flash","limit":{"context":128000,"output":8192},"modalities":{"input":["text"]},"tool_call":true,"structured_output":false,"cost":{"input":0.1,"output":0.2}}}}}"#;
    let report = model_import::import_json(&pool, json, false).await.unwrap();
    assert_eq!(report.counts.new, 1, "{report:?}");
    let id: String =
        sqlx::query_scalar("SELECT id FROM model_registry WHERE id='deepseek-v4-flash'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(id, "deepseek-v4-flash");
}

#[tokio::test]
async fn tied_candidates_prefer_cost_metadata_then_lexicographic_order() {
    let (pool, _dir) = setup().await;
    seed(&pool).await;
    // Two equally scored exact matches: "aaa" wins over "zzz" (lexicographic),
    // but "zzz" has cost metadata and must win regardless of order.
    let json = r#"
        {"aaa":{"models":{"dest":{"id":"grok-4.5_oa","name":"Aaa Dest","limit":{"context":1000,"output":1000}}}},
         "zzz":{"models":{"dest":{"id":"grok-4.5_oa","name":"Zzz Dest","limit":{"context":2000,"output":2000},"cost":{"input":1.0,"output":2.0}}}}}"#;
    let report = model_import::import_json(&pool, json, false).await.unwrap();
    assert_eq!(report.counts.new, 1, "{report:?}");
    let name: String =
        sqlx::query_scalar("SELECT display_name FROM model_registry WHERE id='grok-4.5_oa'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(name, "Zzz Dest");
}

#[tokio::test]
async fn tied_candidates_without_cost_are_idempotent_across_reimport() {
    let (pool, _dir) = setup().await;
    seed(&pool).await;
    // Both exact matches lack cost metadata; lexicographic tie-break must pick
    // the same provider every run so repeated imports are stable.
    let json = r#"
        {"mmm":{"models":{"dest":{"id":"grok-4.5_oa","name":"Mmm Dest","limit":{"context":1000,"output":1000}}}},
         "bbb":{"models":{"dest":{"id":"grok-4.5_oa","name":"Bbb Dest","limit":{"context":2000,"output":2000}}}}}"#;
    model_import::import_json(&pool, json, false).await.unwrap();
    let name: String =
        sqlx::query_scalar("SELECT display_name FROM model_registry WHERE id='grok-4.5_oa'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(name, "Bbb Dest");
    model_import::import_json(&pool, json, true).await.unwrap();
    let name: String =
        sqlx::query_scalar("SELECT display_name FROM model_registry WHERE id='grok-4.5_oa'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(name, "Bbb Dest");
}

#[tokio::test]
async fn pi_array_catalog_is_rejected() {
    let (pool, _dir) = setup().await;
    let json = r#"{"xai":{"models":[{"id":"grok-4.5","name":"Grok 4.5"}]}}"#;
    let error = model_import::import_json(&pool, json, false)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("models must be an object"));
}

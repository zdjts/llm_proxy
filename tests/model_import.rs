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
        r#"{{"xai":{{"models":[{{"id":"grok-4.5","name":"Grok 4.5","contextWindow":500000,"maxTokens":500000,"input":["text","image"],"reasoning":true,"cost":{{"input":2.0,"output":6.0}},"compat":{{"supportsTools":true,"apiKey":"do-not-store"}}{extra}}}]}}}}"#
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
    let bad = source(",\"cost\":{\"input\":-1,\"output\":6}");
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
async fn duplicate_source_ids_are_rejected_globally() {
    let (pool, _dir) = setup().await;
    let json = r#"{"xai":{"models":[{"id":"same"}]},"openai":{"models":[{"id":"same"}]}}"#;
    let error = model_import::import_json(&pool, json, false)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("duplicate source model id"));
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
                keys: vec![llm_proxy::config::KeyEntry {
                    key: "test".into(),
                    weight: 1,
                }],
                strategy: llm_proxy::config::PoolStrategy::WeightedRandom,
            },
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

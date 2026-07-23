use std::path::Path;

#[test]
fn it_loads_fixture_config_successfully() {
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/config.yaml"
    ));
    let config = llm_proxy::config::Config::load(path).unwrap();
    assert_eq!(config.server.port, 8080);
    assert_eq!(config.auth.client_keys.len(), 1);
    assert_eq!(config.pools.len(), 2);
    assert_eq!(config.providers.len(), 2);
    assert_eq!(config.model_to_pool.len(), 4);
}

#[test]
fn it_rejects_orphan_pool_reference() {
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/config.yaml"
    ));
    let mut config = llm_proxy::config::Config::load(path).unwrap();
    config
        .model_to_pool
        .insert("test".into(), "nonexistent".into());
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("nonexistent"));
}

#[test]
fn it_rejects_missing_config_file() {
    let result = llm_proxy::config::Config::load(Path::new("/nonexistent/path/config.yaml"));
    assert!(result.is_err());
}

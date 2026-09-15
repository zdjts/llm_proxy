//! Runtime construction helpers for the DB-backed provider/router snapshot.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::{ModelRouting, PoolConfig, ProviderConfig};
use crate::config_store::ConfigStore;
use crate::error::AppError;
use crate::provider::registry::ProviderRegistry;
use crate::router::{BadKeyRegistry, Router};

/// Build a fresh router from one consistent ConfigStore snapshot.
///
/// Providers are created through the registry's trait-object factories. A
/// missing pool is rejected before the new router is returned, allowing the
/// caller to retain its previous live router.
pub async fn rebuild_router(
    store: &ConfigStore,
    registry: &ProviderRegistry,
    bad_status_codes: Arc<[u16]>,
    bad_keys: Arc<BadKeyRegistry>,
) -> Result<Arc<Router>, AppError> {
    let snapshot = store.snapshot().await;
    let providers = registry
        .build_all(&snapshot.providers, bad_status_codes)
        .await?;
    let pools = snapshot.pool_configs;
    let routing = snapshot.model_routing;

    for (model, route) in routing.iter() {
        if !pools.contains_key(route.pool_id()) {
            return Err(AppError::Config(format!(
                "pool '{}' referenced by model '{model}' not found in configured pools",
                route.pool_id()
            )));
        }
    }

    let mut model_map: HashMap<
        String,
        (
            String,
            PoolConfig,
            Option<serde_json::Value>,
            Option<String>,
        ),
    > = HashMap::new();
    for (model, route) in routing.iter() {
        let pool_id = route.pool_id().to_owned();
        let pool = pools
            .get(&pool_id)
            .cloned()
            .ok_or_else(|| AppError::Config(format!("pool '{pool_id}' not found")))?;
        model_map.insert(
            model.clone(),
            (
                pool_id,
                pool,
                route.default_params().cloned(),
                route.upstream_model().map(str::to_owned),
            ),
        );
    }

    Ok(Arc::new(Router::new(model_map, providers, bad_keys)))
}

/// Snapshot of the ConfigStore sections that affect routing.
pub type RoutingSnapshot = (
    Arc<Vec<ProviderConfig>>,
    Arc<HashMap<String, PoolConfig>>,
    Arc<HashMap<String, ModelRouting>>,
);

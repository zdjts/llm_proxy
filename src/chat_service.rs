//! Core chat routing boundary.
//!
//! The HTTP handler owns extraction and response adaptation; this service owns
//! obtaining a short-lived live router snapshot and routing primitives used by
//! the retry loop. It intentionally does not retain AppState or HTTP types.

use std::sync::Arc;

use crate::config::PoolConfig;
use crate::error::AppError;
use crate::provider::Provider;
use crate::router::{ResolveResult, Router, RouterHandle};

#[derive(Clone)]
pub struct ChatCompletionService {
    router: RouterHandle,
}

impl ChatCompletionService {
    pub fn new(router: RouterHandle) -> Self {
        Self { router }
    }

    /// Take one live router snapshot. The handle lock is not held across await.
    pub fn snapshot(&self) -> Arc<Router> {
        self.router.current()
    }

    /// Resolve a model using a caller-owned snapshot.
    pub fn resolve<'a>(
        &self,
        router: &'a Router,
        model: &str,
    ) -> Result<ResolveResult<'a>, AppError> {
        router.resolve(model)
    }

    pub fn pick_key(
        &self,
        router: &Router,
        pool: &PoolConfig,
        pool_id: &str,
    ) -> Option<crate::config::KeyEntry> {
        router.pick_key(pool, pool_id)
    }

    pub fn mark_bad(&self, router: &Router, pool_id: &str, key: &crate::config::KeyEntry) {
        router.mark_bad(pool_id, key);
    }

    pub fn provider_for<'a>(
        &self,
        result: ResolveResult<'a>,
    ) -> (
        Arc<dyn Provider>,
        &'a PoolConfig,
        &'a str,
        Option<&'a serde_json::Value>,
        Option<&'a str>,
    ) {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::config::{KeyEntry, PoolConfig};
    use crate::router::BadKeyRegistry;

    #[test]
    fn service_snapshot_resolves_unknown_model_without_app_state() {
        let router = Arc::new(Router::new(
            HashMap::new(),
            HashMap::new(),
            Arc::new(BadKeyRegistry::new()),
        ));
        let service = ChatCompletionService::new(RouterHandle::new(router));
        let snapshot = service.snapshot();
        let result = service.resolve(&snapshot, "missing");
        assert!(matches!(result, Err(AppError::NotFound(_))));
    }

    #[test]
    fn service_can_select_a_key_from_a_live_snapshot() {
        let pool = PoolConfig {
            keys: vec![KeyEntry::api_key("test-key", 1)],
        };
        let mut models = HashMap::new();
        models.insert("model".into(), ("pool".into(), pool.clone(), None, None));
        let router = Arc::new(Router::new(
            models,
            HashMap::new(),
            Arc::new(BadKeyRegistry::new()),
        ));
        let service = ChatCompletionService::new(RouterHandle::new(router));
        let snapshot = service.snapshot();
        assert_eq!(
            service.pick_key(&snapshot, &pool, "pool").unwrap().key,
            "test-key"
        );
    }
}

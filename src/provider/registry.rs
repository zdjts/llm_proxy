//! Provider plugin registry (Module B1 — v2.0).
//!
//! Decouples provider instantiation from the main boot sequence through
//! a `ProviderFactory` trait and compile-time `ProviderRegistry`.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::config::ProviderConfig;
use crate::provider::Provider;

#[async_trait]
pub trait ProviderFactory: Send + Sync {
    fn kind_name(&self) -> &str;

    async fn create(
        &self,
        config: &ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn Provider>, crate::error::AppError>;

    fn supports(&self, kind: &crate::config::ProviderKind) -> bool;
}

#[derive(Default)]
pub struct ProviderRegistry {
    factories: Vec<Arc<dyn ProviderFactory>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            factories: Vec::new(),
        }
    }

    pub fn register(&mut self, factory: Arc<dyn ProviderFactory>) {
        self.factories.push(factory);
    }

    pub async fn build_all(
        &self,
        configs: &[ProviderConfig],
        bad_status_codes: Arc<[u16]>,
    ) -> Result<HashMap<String, Arc<dyn Provider>>, crate::error::AppError> {
        let mut providers = HashMap::new();
        for cfg in configs {
            let provider = self.build_one(cfg, Arc::clone(&bad_status_codes)).await?;
            providers.insert(cfg.pool_id.clone(), provider);
        }
        Ok(providers)
    }

    pub async fn build_one(
        &self,
        config: &ProviderConfig,
        bad_status_codes: Arc<[u16]>,
    ) -> Result<Arc<dyn Provider>, crate::error::AppError> {
        for factory in &self.factories {
            if factory.supports(&config.kind) {
                return factory.create(config, bad_status_codes).await;
            }
        }
        Err(crate::error::AppError::Config(format!(
            "no provider factory found for kind '{:?}'",
            config.kind
        )))
    }
}

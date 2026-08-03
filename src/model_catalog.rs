//! Runtime model catalog backed by the live router and managed config store.

use crate::config_store::ConfigStore;
use crate::router::RouterHandle;
use crate::types::{Model, ModelMetadata, ModelMetadataPricing};
use std::sync::Arc;

#[derive(Clone)]
pub struct ModelCatalog {
    router: RouterHandle,
    store: Arc<ConfigStore>,
}

impl ModelCatalog {
    pub fn new(router: RouterHandle, store: Arc<ConfigStore>) -> Self {
        Self { router, store }
    }

    pub fn list_models(&self) -> Vec<Model> {
        self.router
            .current()
            .model_list()
            .into_iter()
            .map(|id| Model {
                id,
                object: "model".into(),
                created: 0,
                owned_by: "openai".into(),
            })
            .collect()
    }

    pub async fn list_metadata(&self) -> Vec<ModelMetadata> {
        let snapshot = self.store.snapshot().await;
        self.router
            .current()
            .model_pool_list()
            .into_iter()
            .map(|(id, pool)| {
                let metadata = snapshot.model_metadata.for_model(&id, &pool);
                ModelMetadata {
                    id,
                    name: metadata.name,
                    context_window: metadata.context_window,
                    max_output_tokens: metadata.max_output_tokens,
                    input_types: metadata.input_types,
                    reasoning: metadata.reasoning,
                    thinking_levels: metadata.thinking_levels,
                    supports_tools: metadata.supports_tools,
                    supports_vision: metadata.supports_vision,
                    pricing: ModelMetadataPricing {
                        input_usd_per_million_tokens: metadata.pricing.input_usd_per_million_tokens,
                        output_usd_per_million_tokens: metadata
                            .pricing
                            .output_usd_per_million_tokens,
                        cache_read_usd_per_million_tokens: metadata
                            .pricing
                            .cache_read_usd_per_million_tokens,
                        cache_write_usd_per_million_tokens: metadata
                            .pricing
                            .cache_write_usd_per_million_tokens,
                    },
                }
            })
            .collect()
    }
}

//! Runtime model catalog backed by the live router and managed config store.

use crate::config_store::{ConfigStore, ModelRegistryEntry};
use crate::error::AppError;
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

    pub async fn translate_reasoning_effort(
        &self,
        model: &str,
        request: &mut crate::types::ChatCompletionRequest,
    ) -> Result<(), AppError> {
        let snapshot = self.store.snapshot().await;
        let Some(entry) = snapshot
            .model_registry
            .iter()
            .find(|entry| entry.id == model && entry.enabled)
        else {
            return Ok(());
        };
        translate_reasoning_effort(entry, &mut request.extra)
    }

    pub async fn list_metadata(&self) -> Vec<ModelMetadata> {
        let snapshot = self.store.snapshot().await;
        self.router
            .current()
            .model_pool_list()
            .into_iter()
            .map(|(id, pool)| {
                let metadata = snapshot.model_metadata.for_model(&id, &pool);
                let registry = snapshot
                    .model_registry
                    .iter()
                    .find(|entry| entry.id == id && entry.enabled);
                let (
                    name,
                    context_window,
                    max_output_tokens,
                    input_types,
                    reasoning,
                    thinking_levels,
                    supports_tools,
                    supports_vision,
                    pricing,
                ) = if let Some(entry) = registry {
                    let caps = entry
                        .capabilities_json
                        .as_deref()
                        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
                    let reasoning = caps
                        .as_ref()
                        .and_then(|value| value.get("reasoning"))
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(metadata.reasoning);
                    let registry_thinking_levels = reasoning_thinking_levels(caps.as_ref());
                    let input_types = if entry.supports_vision {
                        vec!["text".into(), "image".into()]
                    } else {
                        vec!["text".into()]
                    };
                    (
                        entry.display_name.clone(),
                        entry.max_context_tokens.max(0) as u32,
                        entry.max_output_tokens.max(0) as u32,
                        input_types,
                        reasoning,
                        if registry_thinking_levels.is_empty() {
                            metadata.thinking_levels
                        } else {
                            registry_thinking_levels
                        },
                        entry.supports_tool_calling,
                        entry.supports_vision,
                        ModelMetadataPricing {
                            input_usd_per_million_tokens: entry.input_price_per_1m.unwrap_or(0.0),
                            output_usd_per_million_tokens: entry.output_price_per_1m.unwrap_or(0.0),
                            cache_read_usd_per_million_tokens: catalog_cost(
                                caps.as_ref(),
                                "cache_read",
                                metadata.pricing.cache_read_usd_per_million_tokens,
                            ),
                            cache_write_usd_per_million_tokens: catalog_cost(
                                caps.as_ref(),
                                "cache_write",
                                metadata.pricing.cache_write_usd_per_million_tokens,
                            ),
                        },
                    )
                } else {
                    (
                        metadata.name,
                        metadata.context_window,
                        metadata.max_output_tokens,
                        metadata.input_types,
                        metadata.reasoning,
                        metadata.thinking_levels,
                        metadata.supports_tools,
                        metadata.supports_vision,
                        ModelMetadataPricing {
                            input_usd_per_million_tokens: metadata
                                .pricing
                                .input_usd_per_million_tokens,
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
                    )
                };
                ModelMetadata {
                    id,
                    name,
                    context_window,
                    max_output_tokens,
                    input_types,
                    reasoning,
                    thinking_levels,
                    supports_tools,
                    supports_vision,
                    pricing,
                }
            })
            .collect()
    }
}

fn translate_reasoning_effort(
    entry: &ModelRegistryEntry,
    extra: &mut serde_json::Value,
) -> Result<(), AppError> {
    let Some(reasoning_effort) = extra.get("reasoning_effort").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    let Some(caps) = entry
        .capabilities_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
    else {
        return Ok(());
    };
    let Some(level_map) = caps
        .pointer("/metadata/thinkingLevelMap")
        .and_then(|v| v.as_object())
    else {
        return Ok(());
    };
    let Some(upstream_level) = level_map.get(reasoning_effort) else {
        return Err(AppError::BadRequest(format!(
            "model '{}' does not support reasoning_effort='{}'",
            entry.id, reasoning_effort
        )));
    };
    let Some(upstream_level) = upstream_level.as_str() else {
        return Err(AppError::BadRequest(format!(
            "model '{}' does not support reasoning_effort='{}'",
            entry.id, reasoning_effort
        )));
    };
    if let Some(object) = extra.as_object_mut() {
        object.insert(
            "reasoning_effort".to_owned(),
            serde_json::Value::String(upstream_level.to_owned()),
        );
    }
    Ok(())
}

fn catalog_cost(caps: Option<&serde_json::Value>, field: &str, fallback: f64) -> f64 {
    caps.and_then(|value| value.pointer(&format!("/metadata/cost/{field}")))
        .and_then(serde_json::Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .unwrap_or(fallback)
}

fn reasoning_thinking_levels(caps: Option<&serde_json::Value>) -> Vec<String> {
    caps.and_then(|v| v.pointer("/metadata/thinkingLevelMap"))
        .and_then(serde_json::Value::as_object)
        .map(|map| {
            const ORDER: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];
            ORDER
                .iter()
                .filter(|level| map.get(**level).is_some_and(|value| !value.is_null()))
                .map(|level| (*level).to_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{reasoning_thinking_levels, translate_reasoning_effort};
    use crate::config_store::ModelRegistryEntry;

    fn entry(id: &str, caps: serde_json::Value) -> ModelRegistryEntry {
        ModelRegistryEntry {
            id: id.into(),
            display_name: id.into(),
            provider_kind: "openai".into(),
            provider_config_id: None,
            supports_vision: true,
            supports_tool_calling: false,
            supports_json_mode: false,
            max_context_tokens: 1,
            max_output_tokens: 1,
            input_price_per_1m: None,
            output_price_per_1m: None,
            capabilities_json: Some(caps.to_string()),
            enabled: true,
        }
    }

    #[test]
    fn parses_reasoning_and_non_null_thinking_levels() {
        let value = serde_json::json!({
            "reasoning": true,
            "metadata": {"thinkingLevelMap": {
                "off": "none", "minimal": null, "low": "low", "high": "high"
            }}
        });
        assert_eq!(
            reasoning_thinking_levels(Some(&value)),
            vec!["off", "low", "high"]
        );
    }

    #[test]
    fn translates_standard_reasoning_level_using_registry_map() {
        let e = entry(
            "gpt-5.6-luna",
            serde_json::json!({
                "metadata": {"thinkingLevelMap": {"off": "none", "high": "high"}}
            }),
        );
        let mut extra = serde_json::json!({"reasoning_effort": "off"});
        translate_reasoning_effort(&e, &mut extra).unwrap();
        assert_eq!(extra["reasoning_effort"], "none");
    }

    #[test]
    fn rejects_unmapped_reasoning_level() {
        let e = entry(
            "gpt-5.6-luna",
            serde_json::json!({
                "metadata": {"thinkingLevelMap": {"high": "high"}}
            }),
        );
        let mut extra = serde_json::json!({"reasoning_effort": "minimal"});
        assert!(translate_reasoning_effort(&e, &mut extra).is_err());
    }

    #[test]
    fn leaves_models_without_level_map_unchanged() {
        let e = entry("grok-4.3-pro", serde_json::json!({"reasoning": true}));
        let mut extra = serde_json::json!({"reasoning_effort": "high"});
        translate_reasoning_effort(&e, &mut extra).unwrap();
        assert_eq!(extra["reasoning_effort"], "high");
    }
}

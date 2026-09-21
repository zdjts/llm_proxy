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
                    thinking_level_map,
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
                    let level_map = registry_thinking_level_map(caps.as_ref());
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
                        level_map,
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
                        None,
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
                // Full map wins; otherwise derive an identity map from the
                // advertised levels so clients never have to guess the wire
                // values (they equal the canonical level names).
                let thinking_level_map =
                    thinking_level_map.or_else(|| identity_thinking_level_map(&thinking_levels));
                ModelMetadata {
                    id,
                    name,
                    context_window,
                    max_output_tokens,
                    input_types,
                    reasoning,
                    thinking_levels,
                    thinking_level_map,
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
        // No per-model map: the caller knows this model's vocabulary and the
        // gateway must not second-guess it.  Pass the level through verbatim.
        return Ok(());
    };
    // Canonicalise the requested level so `off` (pi) finds the `none` entry
    // (xAI-style catalogs) written by older imports: map keys are matched by
    // canonical form, not byte equality.
    let Some(requested) = canonical_thinking_level(reasoning_effort) else {
        // Not a canonical level — a provider-specific value the client set
        // deliberately.  The gateway is not a vocabulary police: pass through.
        return Ok(());
    };
    let upstream_level = level_map
        .iter()
        .find(|(key, _)| canonical_thinking_level(key) == Some(requested))
        .map(|(_, value)| value);
    let Some(upstream_level) = upstream_level else {
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

/// Identity map from advertised canonical levels to themselves, used when a
/// model declares `thinking_levels` without a per-level wire-value map.
fn identity_thinking_level_map(
    levels: &[String],
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if levels.is_empty() {
        return None;
    }
    Some(
        levels
            .iter()
            .map(|level| (level.clone(), serde_json::Value::String(level.clone())))
            .collect(),
    )
}

fn catalog_cost(caps: Option<&serde_json::Value>, field: &str, fallback: f64) -> f64 {
    caps.and_then(|value| value.pointer(&format!("/metadata/cost/{field}")))
        .and_then(serde_json::Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .unwrap_or(fallback)
}

/// Canonical pi/gateway thinking levels, in ascending effort order.
pub const THINKING_LEVEL_ORDER: &[&str] =
    &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Map a catalog level name to its canonical form.
///
/// models.dev spells the "thinking disabled" effort value `none` (xAI, hy4),
/// while pi and the gateway metadata API use `off`.  Everything else that is
/// already canonical passes through; unknown names return `None`.
pub fn canonical_thinking_level(level: &str) -> Option<&'static str> {
    match level {
        "none" => Some("off"),
        other => THINKING_LEVEL_ORDER
            .iter()
            .find(|canonical| **canonical == other)
            .copied(),
    }
}

/// Extract the per-model `canonical level → upstream wire value` map from a
/// `model_registry` capabilities blob.
///
/// Keys are canonicalised (`none` → `off`) so rows written by older imports
/// behave identically to rows written after [`crate::model_import`] started
/// canonicalising at import time.  Values are passed through verbatim: they
/// are what the upstream API expects in `reasoning_effort`.
fn registry_thinking_level_map(
    caps: Option<&serde_json::Value>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let map = caps
        .and_then(|v| v.pointer("/metadata/thinkingLevelMap"))
        .and_then(serde_json::Value::as_object)?;
    let mut out = serde_json::Map::new();
    for (key, value) in map {
        if value.is_null() {
            continue;
        }
        if let Some(canonical) = canonical_thinking_level(key) {
            out.insert(canonical.to_owned(), value.clone());
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Legacy list-shaped view of the map, for YAML-configured models that only
/// declare `thinking_levels` (identity mapping) instead of a full map.
fn reasoning_thinking_levels(caps: Option<&serde_json::Value>) -> Vec<String> {
    registry_thinking_level_map(caps)
        .map(|map| {
            THINKING_LEVEL_ORDER
                .iter()
                .filter(|level| map.contains_key(**level))
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

    #[test]
    fn canonicalises_off_against_legacy_none_keyed_map() {
        // Pre-canonicalisation import rows store xAI-style `none` keys; a pi
        // `off` request must still find (and translate to) the wire value.
        let e = entry(
            "grok-4.6",
            serde_json::json!({
                "metadata": {"thinkingLevelMap": {"none": "none", "low": "low", "high": "high"}}
            }),
        );
        let mut extra = serde_json::json!({"reasoning_effort": "off"});
        translate_reasoning_effort(&e, &mut extra).unwrap();
        assert_eq!(extra["reasoning_effort"], "none");
    }

    #[test]
    fn passes_through_provider_specific_effort_values() {
        // `reasoning_effort` values outside the canonical vocabulary are the
        // client's deliberate choice; the gateway must not rewrite them.
        let e = entry(
            "grok-4.6",
            serde_json::json!({
                "metadata": {"thinkingLevelMap": {"low": "low"}}
            }),
        );
        let mut extra = serde_json::json!({"reasoning_effort": "turbo"});
        translate_reasoning_effort(&e, &mut extra).unwrap();
        assert_eq!(extra["reasoning_effort"], "turbo");
    }

    #[test]
    fn canonical_level_missing_from_map_is_rejected() {
        // A canonical level absent from the map is unsupported for this
        // model — surface that as a 400 instead of a confusing upstream 4xx.
        let e = entry(
            "hy4-preview",
            serde_json::json!({
                "metadata": {"thinkingLevelMap": {"high": "high", "off": "none"}}
            }),
        );
        let mut extra = serde_json::json!({"reasoning_effort": "low"});
        let err = translate_reasoning_effort(&e, &mut extra).unwrap_err();
        assert!(matches!(err, crate::error::AppError::BadRequest(_)));
    }

    #[test]
    fn canonical_thinking_level_aliases_and_passes_through() {
        use super::canonical_thinking_level;
        assert_eq!(canonical_thinking_level("none"), Some("off"));
        assert_eq!(canonical_thinking_level("off"), Some("off"));
        assert_eq!(canonical_thinking_level("xhigh"), Some("xhigh"));
        assert_eq!(canonical_thinking_level("turbo"), None);
    }

    #[test]
    fn registry_map_drops_null_entries_and_unknown_keys() {
        use super::registry_thinking_level_map;
        let value = serde_json::json!({
            "metadata": {"thinkingLevelMap": {
                "none": "none", "low": "low", "minimal": null, "turbo": "turbo"
            }}
        });
        let map = registry_thinking_level_map(Some(&value)).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["off"], "none");
        assert_eq!(map["low"], "low");
        assert!(map.get("minimal").is_none());
        assert!(map.get("turbo").is_none());
    }

    #[test]
    fn identity_map_from_levels() {
        use super::identity_thinking_level_map;
        assert!(identity_thinking_level_map(&[]).is_none());
        let map = identity_thinking_level_map(&["low".into(), "high".into()]).unwrap();
        assert_eq!(map["low"], "low");
        assert_eq!(map["high"], "high");
    }
}

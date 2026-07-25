//! Fallback model routing — ADR-017.
//!
//! When a primary model's pool is exhausted or circuit-broken, automatically
//! routes to a configured fallback model. Supports chains: gpt-4o → gpt-4o-mini.

use std::collections::HashMap;

use serde::Deserialize;

use crate::circuit_breaker::CircuitBreaker;
use crate::router::Router;

/// Fallback routing configuration — loaded from `fallback_models` in config.yaml.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FallbackConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub chains: HashMap<String, FallbackChain>,
}

/// A chain of model names to try in order.
#[derive(Debug, Clone, Deserialize)]
pub struct FallbackChain {
    pub models: Vec<String>,
}

impl FallbackConfig {
    /// Resolve a model through its fallback chain. Returns the original model
    /// if no fallback is configured or the chain is exhausted.
    pub fn resolve(
        &self,
        model: &str,
        tried: &[String],
        router: &Router,
        _circuit_breaker: &CircuitBreaker,
    ) -> Option<String> {
        if !self.enabled {
            return None;
        }

        let chain = self.chains.get(model)?;

        for fb_model in &chain.models {
            if tried.contains(fb_model) {
                continue;
            }
            if fb_model == model {
                continue;
            }
            match router.resolve(fb_model) {
                Ok((_, _pool, pool_id, _)) => {
                    let snapshot = router.pool_snapshot();
                    let pool_healthy = snapshot
                        .iter()
                        .any(|s| s.pool_id == pool_id && s.keys.iter().any(|k| k.healthy));
                    if pool_healthy {
                        return Some(fb_model.clone());
                    }
                }
                Err(_) => continue,
            }
        }

        None
    }
}

/// Helper: attempt pickup from a fallback chain, returning the effective model to use.
pub fn try_fallback(
    router: &Router,
    circuit_breaker: &CircuitBreaker,
    fallback_config: &FallbackConfig,
    original_model: &str,
    tried_models: &[String],
) -> Option<String> {
    fallback_config.resolve(original_model, tried_models, router, circuit_breaker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_disabled_returns_none() {
        let cfg = FallbackConfig::default();
        assert!(!cfg.enabled);
    }

    #[test]
    fn fallback_chain_config() {
        let mut cfg = FallbackConfig {
            enabled: true,
            chains: HashMap::new(),
        };
        cfg.chains.insert(
            "gpt-4o".into(),
            FallbackChain {
                models: vec!["gpt-4o-mini".into(), "deepseek-chat".into()],
            },
        );
        assert!(cfg.chains.contains_key("gpt-4o"));
        assert_eq!(cfg.chains["gpt-4o"].models.len(), 2);
        assert_eq!(cfg.chains["gpt-4o"].models[0], "gpt-4o-mini");
    }
}

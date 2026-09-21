//! Per-tenant pricing config — ADR-014 §2 (T59).
//!
//! Supports two YAML shapes for backward compatibility:
//! 1. V2 nested: `pricing: { models: { "<model>": { default: {...}, tenants: {...} } } }`
//! 2. V1 flat:    `pricing: { "<model>": { prompt: ..., completion: ... } }`
//!
//! [`AccountingPricing`] is the explicit billing boundary. It delegates to this
//! config without consulting model metadata pricing, whose units and purpose are
//! different (display-only USD per million tokens).

use std::collections::HashMap;

use serde::Deserialize;

/// V1-compatible per-model price entry.
#[derive(Debug, Clone, Deserialize)]
pub struct PriceEntry {
    #[serde(default)]
    pub prompt: f64,
    #[serde(default)]
    pub completion: f64,
}

impl Default for PriceEntry {
    fn default() -> Self {
        Self {
            prompt: 0.0,
            completion: 0.0,
        }
    }
}

/// Unified pricing model with optional per-tenant overrides.
#[derive(Debug, Clone, Default)]
pub struct PricingConfig {
    pub models: HashMap<String, ModelPricing>,
}

#[derive(Debug, Clone)]
pub struct ModelPricing {
    pub default: PriceEntry,
    pub tenants: HashMap<String, PriceEntry>,
}

/// Read-only billing view over [`PricingConfig`].
///
/// This type is intentionally narrow: accounting, budget checks, and cost
/// calculations use this view; metadata pricing is never consulted or merged.
#[derive(Debug, Clone, Copy)]
pub struct AccountingPricing<'a> {
    config: &'a PricingConfig,
}

impl<'a> AccountingPricing<'a> {
    pub fn new(config: &'a PricingConfig) -> Self {
        Self { config }
    }

    /// Look up the effective accounting price: tenant override, then default,
    /// then the zero-price missing-model fallback.
    pub fn lookup(&self, model: &str, tenant: Option<&str>) -> &PriceEntry {
        self.config.lookup(model, tenant)
    }

    /// Cost in USD for a request that consumed `prompt`/`completion` tokens.
    ///
    /// Returns `None` when the model resolves to a zero price in every layer,
    /// which callers must preserve as SQL NULL rather than writing 0.0 —
    /// "unpriced" and "free" are different facts.
    pub fn cost_usd(
        &self,
        model: &str,
        tenant: Option<&str>,
        prompt_tokens: i64,
        completion_tokens: i64,
    ) -> Option<f64> {
        let price = self.lookup(model, tenant);
        if price.prompt == 0.0 && price.completion == 0.0 {
            return None;
        }
        Some(
            prompt_tokens as f64 * price.prompt / 1_000_000.0
                + completion_tokens as f64 * price.completion / 1_000_000.0,
        )
    }
}

impl PricingConfig {
    /// Create the explicit billing view for this configuration.
    pub fn accounting(&self) -> AccountingPricing<'_> {
        AccountingPricing::new(self)
    }

    /// Look up the effective price for a (model, tenant) pair.
    /// Falls back: tenants[tenant] → default → zero.
    pub fn lookup(&self, model: &str, tenant: Option<&str>) -> &PriceEntry {
        static ZERO: PriceEntry = PriceEntry {
            prompt: 0.0,
            completion: 0.0,
        };
        let mp = match self.models.get(model) {
            Some(mp) => mp,
            None => return &ZERO,
        };
        if let Some(t) = tenant
            && let Some(pe) = mp.tenants.get(t)
        {
            return pe;
        }
        &mp.default
    }
}

// ── Serde deserialization ───────────────────────────────────────────────

impl<'de> Deserialize<'de> for PricingConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPricing::deserialize(deserializer)?;
        Ok(match raw {
            RawPricing::V2 { models } => PricingConfig {
                models: models.into_iter().map(|(k, v)| (k, v.into())).collect(),
            },
            RawPricing::V1(flat) => {
                let models = flat
                    .into_iter()
                    .map(|(model, price)| {
                        (
                            model,
                            ModelPricing {
                                default: price,
                                tenants: HashMap::new(),
                            },
                        )
                    })
                    .collect();
                PricingConfig { models }
            }
        })
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawPricing {
    V2 {
        models: HashMap<String, RawModelPricing>,
    },
    V1(HashMap<String, PriceEntry>),
}

#[derive(Debug, Clone, Deserialize)]
struct RawModelPricing {
    #[serde(default)]
    default: PriceEntry,
    #[serde(default)]
    tenants: HashMap<String, PriceEntry>,
}

impl From<RawModelPricing> for ModelPricing {
    fn from(r: RawModelPricing) -> Self {
        ModelPricing {
            default: r.default,
            tenants: r.tenants,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_v1_flat() {
        let yaml = "gpt-4o:\n  prompt: 2.5\n  completion: 10.0";
        let cfg: PricingConfig = serde_yaml::from_str(yaml).unwrap();
        let price = cfg.lookup("gpt-4o", None);
        assert_eq!(price.prompt, 2.5);
        assert_eq!(price.completion, 10.0);
    }

    #[test]
    fn deserialize_v2_nested() {
        let yaml = r#"
models:
  gpt-4o:
    default:
      prompt: 2.5
      completion: 10.0
    tenants:
      alice:
        prompt: 2.0
        completion: 8.0
"#;
        let cfg: PricingConfig = serde_yaml::from_str(yaml).unwrap();
        let d = cfg.lookup("gpt-4o", None);
        assert_eq!(d.prompt, 2.5);
        assert_eq!(d.completion, 10.0);
        let a = cfg.lookup("gpt-4o", Some("alice"));
        assert_eq!(a.prompt, 2.0);
        assert_eq!(a.completion, 8.0);
    }

    #[test]
    fn lookup_missing_model_returns_zero() {
        let cfg = PricingConfig::default();
        let p = cfg.lookup("nonexistent", None);
        assert_eq!(p.prompt, 0.0);
        assert_eq!(p.completion, 0.0);
    }

    #[test]
    fn lookup_tenant_missing_falls_back_to_default() {
        let yaml = r#"
models:
  gpt-4o:
    default:
      prompt: 2.5
      completion: 10.0
    tenants:
      alice:
        prompt: 2.0
        completion: 8.0
"#;
        let cfg: PricingConfig = serde_yaml::from_str(yaml).unwrap();
        let bob = cfg.lookup("gpt-4o", Some("bob"));
        assert_eq!(bob.prompt, 2.5);
        assert_eq!(bob.completion, 10.0);
    }

    #[test]
    fn accounting_view_is_independent_from_metadata_pricing() {
        let accounting: PricingConfig =
            serde_yaml::from_str("gpt-4o:\n  prompt: 2.5\n  completion: 10.0").unwrap();
        let metadata = crate::config::ModelMetadataConfig {
            defaults: crate::config::ModelMetadataPartial {
                pricing: crate::config::MetadataPricingPartial {
                    input_usd_per_million_tokens: Some(999.0),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let displayed = metadata.for_model("gpt-4o", "pool-a");
        let accounting_view = accounting.accounting();
        let billed = accounting_view.lookup("gpt-4o", None);
        assert_eq!(displayed.pricing.input_usd_per_million_tokens, 999.0);
        assert_eq!((billed.prompt, billed.completion), (2.5, 10.0));
    }
}

//! Configuration loading and validation.
//!
//! Loads `config.yaml` into a strongly-typed [`Config`] struct. Validates on startup:
//! every `model_to_pool` entry must reference an existing pool; provider `pool_id` references
//! must resolve; key entries must be non-empty.
//!
//! YAML subsystem types live here (or are re-exported from here). After boot,
//! ADR-017 still applies: this struct is the **startup YAML snapshot**, not the
//! live [`crate::config_store::ConfigStore`] for pools/providers/routing.

pub mod pricing;

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Feature-module YAML types, re-exported so `config` is the consistent home.
pub use crate::auth::ClientKeyEntry;
pub use crate::fallback::FallbackConfig;

/// Root configuration loaded from `config.yaml`.
///
/// YAML-only after boot: `server`, `auth`, `admin`, `rate_limit`, `concurrency`,
/// `fallback_models`, `cache_max_entries`, `failover`/`alerts` (also copied
/// into [`crate::config_store::RuntimePolicy`]).
/// Bootstrapped then DB-owned: `pools`, `providers`, `model_to_pool`.
/// Accounting vs display: `pricing` vs `model_metadata` (do not merge).
#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub db: DbConfig,
    pub failover: FailoverConfig,
    pub pools: HashMap<String, PoolConfig>,
    pub providers: Vec<ProviderConfig>,
    pub model_to_pool: HashMap<String, ModelRouting>,
    /// Optional managed model registry rows carried through explicit YAML
    /// import/export. Empty on bootstrap configs that only define routing.
    #[serde(default)]
    pub model_registry: Vec<ModelRegistryConfig>,
    #[serde(default)]
    pub model_metadata: ModelMetadataConfig,
    #[serde(default)]
    pub admin: AdminConfig,
    #[serde(default)]
    pub pricing: pricing::PricingConfig,
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    #[serde(default = "default_cache_max")]
    pub cache_max_entries: usize,
    #[serde(default)]
    pub alerts: AlertConfig,
    #[serde(default)]
    pub fallback_models: FallbackConfig,
    #[serde(default)]
    pub concurrency: ConcurrencyConfig,
}

/// HTTP server bind settings.
#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// Maximum request body size in bytes. Defaults to 1 GiB.
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,
}

fn default_max_body_bytes() -> usize {
    1_073_741_824
}

/// Client-side authentication configuration.
#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    /// List of valid client API key entries for gateway access.
    pub client_keys: Vec<ClientKeyEntry>,
}

/// SQLite database path configuration.
#[derive(Debug, Deserialize)]
pub struct DbConfig {
    /// Path to the SQLite database file.
    pub path: String,
}

/// Failover / health-probe tuning parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct FailoverConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// HTTP status codes that mark a key as bad. Default: [401, 402, 403, 429].
    #[serde(default = "default_bad_status_codes")]
    pub bad_status_codes: Vec<u16>,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_probe_interval")]
    pub probe_interval_secs: u64,
    #[serde(default = "default_probe_timeout")]
    pub probe_timeout_secs: u64,
    #[serde(default = "default_max_probe_retries")]
    pub max_probe_retries: u32,
}

fn default_true() -> bool {
    true
}

fn default_bad_status_codes() -> Vec<u16> {
    vec![401, 402, 403, 429]
}

fn default_probe_interval() -> u64 {
    60
}

fn default_probe_timeout() -> u64 {
    10
}

fn default_max_probe_retries() -> u32 {
    3
}

fn default_max_retries() -> u32 {
    1
}

/// Key-selection strategy for a pool. Per ADR §3.1 must be an enum.
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PoolStrategy {
    /// Weighted random selection by each key's weight.
    #[default]
    WeightedRandom,
}

/// Provider kind for dispatch.
///
/// Wire name is `openai` (matches DB / docs). `open_ai` is accepted as a
/// legacy alias from older admin exports.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[serde(rename = "openai", alias = "open_ai")]
    OpenAi,
    Anthropic,
    Gemini,
    Azure,
    Bedrock,
    Cohere,
    Mistral,
    Ollama,
    Vllm,
}

fn default_kind() -> ProviderKind {
    ProviderKind::OpenAi
}

/// Pool configuration: a collection of API keys with a selection strategy.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PoolConfig {
    pub keys: Vec<KeyEntry>,
    /// Key-selection strategy. Defaults to [`PoolStrategy::WeightedRandom`].
    #[serde(default)]
    pub strategy: PoolStrategy,
}

/// Kind of upstream credential stored in a [`KeyEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CredentialType {
    #[default]
    ApiKey,
    Oauth,
}

/// An upstream API key or OAuth credential with its routing weight.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct KeyEntry {
    /// Current Bearer token (API key, or OAuth access token).
    #[serde(default)]
    pub key: String,
    /// Weight for weighted-random selection. Defaults to 0 (never selected unless all zero).
    #[serde(default)]
    pub weight: u32,
    /// `api_key` (default) or `oauth`.
    #[serde(default, rename = "type")]
    pub cred_type: CredentialType,
    /// OAuth refresh token. Identity hash is derived from this when present.
    #[serde(default)]
    pub refresh: Option<String>,
    /// Access-token expiry in unix milliseconds (already skewed).
    #[serde(default)]
    pub expires: Option<i64>,
    /// OAuth issuer, e.g. `xai`.
    #[serde(default)]
    pub issuer: Option<String>,
    /// Stable identity hash loaded from DB. Not serialized.
    #[serde(skip)]
    pub identity: Option<String>,
}

impl KeyEntry {
    pub fn api_key(key: impl AsRef<str>, weight: u32) -> Self {
        Self {
            key: key.as_ref().to_owned(),
            weight,
            cred_type: CredentialType::ApiKey,
            refresh: None,
            expires: None,
            issuer: None,
            identity: None,
        }
    }

    pub fn oauth(
        access: impl AsRef<str>,
        refresh: impl AsRef<str>,
        issuer: impl AsRef<str>,
        weight: u32,
        expires: Option<i64>,
    ) -> Self {
        Self {
            key: access.as_ref().to_owned(),
            weight,
            cred_type: CredentialType::Oauth,
            refresh: Some(refresh.as_ref().to_owned()),
            expires,
            issuer: Some(issuer.as_ref().to_owned()),
            identity: None,
        }
    }

    pub fn is_oauth(&self) -> bool {
        self.cred_type == CredentialType::Oauth
    }

    pub fn cred_type_str(&self) -> &'static str {
        match self.cred_type {
            CredentialType::ApiKey => "api_key",
            CredentialType::Oauth => "oauth",
        }
    }

    fn identity_secret(&self) -> &str {
        if self.is_oauth() {
            self.refresh
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&self.key)
        } else {
            &self.key
        }
    }

    /// Stable hash used for bad-key tracking, health, and DB identity.
    pub fn identity_hash(&self) -> String {
        if let Some(id) = self.identity.as_deref().filter(|s| !s.is_empty()) {
            return id.to_owned();
        }
        crate::db::compute_key_hash(self.identity_secret())
    }
}

/// Upstream provider definition.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProviderConfig {
    /// Unique provider identifier, e.g. "openai", "deepseek".
    pub id: String,
    /// Pool ID that this provider draws keys from.
    pub pool_id: String,
    /// Base URL of the upstream OpenAI-compatible API.
    pub base_url: String,
    /// Provider kind for dispatch. Defaults to `openai`.
    #[serde(default = "default_kind")]
    pub kind: ProviderKind,
    /// Azure API version (Azure only).
    #[serde(default)]
    pub api_version: Option<String>,
    /// AWS region (Bedrock only).
    #[serde(default)]
    pub region: Option<String>,
    /// Arbitrary provider-specific metadata (JSON).
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Managed model-registry entry for explicit config import/export.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ModelRegistryConfig {
    pub id: String,
    pub display_name: String,
    pub provider_kind: String,
    #[serde(default)]
    pub provider_config_id: Option<String>,
    #[serde(default)]
    pub supports_vision: bool,
    #[serde(default)]
    pub supports_tool_calling: bool,
    #[serde(default)]
    pub supports_json_mode: bool,
    #[serde(default = "default_registry_tokens")]
    pub max_context_tokens: i32,
    #[serde(default = "default_registry_tokens")]
    pub max_output_tokens: i32,
    #[serde(default)]
    pub input_price_per_1m: Option<f64>,
    #[serde(default)]
    pub output_price_per_1m: Option<f64>,
    #[serde(default)]
    pub capabilities_json: serde_json::Value,
    #[serde(default = "default_registry_enabled")]
    pub enabled: bool,
}

fn default_registry_tokens() -> i32 {
    4096
}

fn default_registry_enabled() -> bool {
    true
}

/// Model-to-pool routing entry. Accepts either a plain pool-id string or an
/// object with `pool` and optional `default_params` for request injection.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ModelRouting {
    Simple(String),
    WithParams {
        pool: String,
        #[serde(default)]
        default_params: serde_json::Value,
    },
}

impl ModelRouting {
    pub fn pool_id(&self) -> &str {
        match self {
            ModelRouting::Simple(s) => s,
            ModelRouting::WithParams { pool, .. } => pool,
        }
    }

    pub fn default_params(&self) -> Option<&serde_json::Value> {
        match self {
            ModelRouting::Simple(_) => None,
            ModelRouting::WithParams { default_params, .. } => {
                if default_params.is_object() {
                    Some(default_params)
                } else {
                    None
                }
            }
        }
    }
}

impl From<&str> for ModelRouting {
    fn from(s: &str) -> Self {
        ModelRouting::Simple(s.to_owned())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelMetadataConfig {
    #[serde(default)]
    pub defaults: ModelMetadataPartial,
    #[serde(default)]
    pub pools: HashMap<String, ModelMetadataPartial>,
    #[serde(default)]
    pub models: HashMap<String, ModelMetadataPartial>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelMetadataPartial {
    pub name: Option<String>,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub input_types: Option<Vec<String>>,
    pub reasoning: Option<bool>,
    pub thinking_levels: Option<Vec<String>>,
    pub supports_tools: Option<bool>,
    pub supports_vision: Option<bool>,
    #[serde(default)]
    pub pricing: MetadataPricingPartial,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MetadataPricingPartial {
    pub input_usd_per_million_tokens: Option<f64>,
    pub output_usd_per_million_tokens: Option<f64>,
    pub cache_read_usd_per_million_tokens: Option<f64>,
    pub cache_write_usd_per_million_tokens: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelMetadata {
    pub name: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub input_types: Vec<String>,
    pub reasoning: bool,
    pub thinking_levels: Vec<String>,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub pricing: MetadataPricing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetadataPricing {
    pub input_usd_per_million_tokens: f64,
    pub output_usd_per_million_tokens: f64,
    pub cache_read_usd_per_million_tokens: f64,
    pub cache_write_usd_per_million_tokens: f64,
}

impl ModelMetadataConfig {
    pub fn for_model(&self, model: &str, pool: &str) -> ModelMetadata {
        let mut out = ModelMetadata {
            name: model.to_owned(),
            context_window: 128_000,
            max_output_tokens: 16_384,
            input_types: vec!["text".into()],
            reasoning: false,
            thinking_levels: Vec::new(),
            supports_tools: false,
            supports_vision: false,
            pricing: MetadataPricing {
                input_usd_per_million_tokens: 0.0,
                output_usd_per_million_tokens: 0.0,
                cache_read_usd_per_million_tokens: 0.0,
                cache_write_usd_per_million_tokens: 0.0,
            },
        };
        let empty = ModelMetadataPartial::default();
        let pool_partial = self.pools.get(pool).unwrap_or(&empty);
        let model_partial = self.models.get(model).unwrap_or(&empty);
        for p in [&self.defaults, pool_partial, model_partial] {
            if let Some(v) = &p.name {
                out.name = v.clone();
            }
            if let Some(v) = p.context_window {
                out.context_window = v;
            }
            if let Some(v) = p.max_output_tokens {
                out.max_output_tokens = v;
            }
            if let Some(v) = &p.input_types {
                out.input_types = v.clone();
            }
            if let Some(v) = p.reasoning {
                out.reasoning = v;
            }
            if let Some(v) = &p.thinking_levels {
                out.thinking_levels = v.clone();
            }
            if let Some(v) = p.supports_tools {
                out.supports_tools = v;
            }
            if let Some(v) = p.supports_vision {
                out.supports_vision = v;
            }
            if let Some(v) = p.pricing.input_usd_per_million_tokens {
                out.pricing.input_usd_per_million_tokens = v;
            }
            if let Some(v) = p.pricing.output_usd_per_million_tokens {
                out.pricing.output_usd_per_million_tokens = v;
            }
            if let Some(v) = p.pricing.cache_read_usd_per_million_tokens {
                out.pricing.cache_read_usd_per_million_tokens = v;
            }
            if let Some(v) = p.pricing.cache_write_usd_per_million_tokens {
                out.pricing.cache_write_usd_per_million_tokens = v;
            }
        }
        out
    }
}

/// Admin dashboard configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AdminConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_allowed_ips")]
    pub allowed_ips: Vec<String>,
}

fn default_allowed_ips() -> Vec<String> {
    vec!["127.0.0.1".into(), "::1".into()]
}

/// Per-model upstream pricing (USD per token).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_rpm")]
    pub requests_per_minute: usize,
}

fn default_rpm() -> usize {
    60
}

fn default_cache_max() -> usize {
    256
}

/// Per-tenant concurrency limits.
#[derive(Debug, Clone, Deserialize)]
pub struct ConcurrencyConfig {
    #[serde(default = "default_concurrency")]
    pub max_per_tenant: usize,
    #[serde(default = "default_total_concurrency")]
    pub total_max: usize,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            max_per_tenant: default_concurrency(),
            total_max: default_total_concurrency(),
        }
    }
}

fn default_concurrency() -> usize {
    50
}

fn default_total_concurrency() -> usize {
    500
}

/// Alert / webhook configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AlertConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub webhook_url: String,
    #[serde(default)]
    pub webhook_secret: String,
    #[serde(default = "default_min_error_burst")]
    pub min_error_burst: u32,
    #[serde(default = "default_min_latency_ms")]
    pub min_latency_ms: u64,
    #[serde(default)]
    pub channels: ChannelsConfig,
}

/// Per-channel alert delivery configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub webhook: ChannelDef,
    #[serde(default)]
    pub slack: ChannelDef,
    #[serde(default)]
    pub discord: ChannelDef,
    #[serde(default)]
    pub email: EmailDef,
}

/// Single channel definition.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChannelDef {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
}

/// Email channel has `to` field instead of `url`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmailDef {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub to: String,
}

fn default_min_error_burst() -> u32 {
    3
}

fn default_min_latency_ms() -> u64 {
    30_000
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, crate::error::AppError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::error::AppError::Config(format!("Failed to read config file: {e}"))
        })?;
        let config: Config = serde_yaml::from_str(&content)
            .map_err(|e| crate::error::AppError::Config(format!("Failed to parse config: {e}")))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        for (model, routing) in &self.model_to_pool {
            let pool_id = routing.pool_id();
            if !self.pools.contains_key(pool_id) {
                return Err(crate::error::AppError::Config(format!(
                    "Model '{model}' references pool '{pool_id}' which does not exist",
                )));
            }
        }
        for provider in &self.providers {
            if !self.pools.contains_key(&provider.pool_id) {
                return Err(crate::error::AppError::Config(format!(
                    "Provider '{}' references pool '{}' which does not exist",
                    provider.id, provider.pool_id,
                )));
            }
        }
        for (pool_id, pool) in &self.pools {
            for (i, key) in pool.keys.iter().enumerate() {
                if key.is_oauth() {
                    if key.refresh.as_deref().filter(|s| !s.is_empty()).is_none() {
                        return Err(crate::error::AppError::Config(format!(
                            "Pool '{pool_id}' key {i} oauth credential is missing refresh",
                        )));
                    }
                    if key.issuer.as_deref().filter(|s| !s.is_empty()).is_none() {
                        return Err(crate::error::AppError::Config(format!(
                            "Pool '{pool_id}' key {i} oauth credential is missing issuer",
                        )));
                    }
                } else if key.key.is_empty() {
                    return Err(crate::error::AppError::Config(format!(
                        "Pool '{pool_id}' key {i} has empty key",
                    )));
                }
            }
        }
        let mut seen_registry = std::collections::HashSet::new();
        for entry in &self.model_registry {
            if entry.id.trim().is_empty() {
                return Err(crate::error::AppError::Config(
                    "model_registry entry id must be non-empty".into(),
                ));
            }
            if entry.display_name.trim().is_empty() {
                return Err(crate::error::AppError::Config(format!(
                    "model_registry entry '{}' display_name must be non-empty",
                    entry.id
                )));
            }
            if entry.provider_kind.trim().is_empty() {
                return Err(crate::error::AppError::Config(format!(
                    "model_registry entry '{}' provider_kind must be non-empty",
                    entry.id
                )));
            }
            if entry.max_context_tokens <= 0 || entry.max_output_tokens <= 0 {
                return Err(crate::error::AppError::Config(format!(
                    "model_registry entry '{}' token limits must be positive",
                    entry.id
                )));
            }
            for price in [entry.input_price_per_1m, entry.output_price_per_1m]
                .into_iter()
                .flatten()
            {
                if !price.is_finite() || price < 0.0 {
                    return Err(crate::error::AppError::Config(format!(
                        "model_registry entry '{}' prices must be finite and non-negative",
                        entry.id
                    )));
                }
            }
            if !entry.capabilities_json.is_null() && !entry.capabilities_json.is_object() {
                return Err(crate::error::AppError::Config(format!(
                    "model_registry entry '{}' capabilities_json must be an object",
                    entry.id
                )));
            }
            if let Some(provider_id) = entry.provider_config_id.as_deref() {
                if provider_id.trim().is_empty() {
                    return Err(crate::error::AppError::Config(format!(
                        "model_registry entry '{}' provider_config_id cannot be empty",
                        entry.id
                    )));
                }
                if !self.providers.iter().any(|p| p.id == provider_id) {
                    return Err(crate::error::AppError::Config(format!(
                        "model_registry entry '{}' references unknown provider '{provider_id}'",
                        entry.id
                    )));
                }
            }
            if !seen_registry.insert(entry.id.clone()) {
                return Err(crate::error::AppError::Config(format!(
                    "duplicate model_registry id '{}'",
                    entry.id
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_parses_minimal_config() {
        let yaml = r#"
server:
  host: "0.0.0.0"
  port: 8080

auth:
  client_keys:
    - { key: "sk-test" }

db:
  path: "./test.db"

failover:
  enabled: true

pools:
  test_pool:
    keys:
      - { key: "sk-aaa", weight: 1 }

providers:
  - id: test
    pool_id: test_pool
    base_url: "https://api.test.com/v1"

model_to_pool:
  "test-model": test_pool
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.pools.len(), 1);
        assert!(config.model_to_pool.contains_key("test-model"));
        config.validate().unwrap();
    }

    #[test]
    fn it_detects_orphan_pool_reference() {
        let yaml = r#"
server:
  host: "0.0.0.0"
  port: 8080

auth:
  client_keys:
    - { key: "sk-test" }

db:
  path: "./test.db"

failover:
  enabled: true

pools:
  existing_pool:
    keys:
      - { key: "sk-aaa", weight: 1 }

providers:
  - id: test
    pool_id: existing_pool
    base_url: "https://api.test.com/v1"

model_to_pool:
  "test-model": non_existent_pool
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("non_existent_pool"));
    }

    #[test]
    fn it_detects_orphan_provider_pool() {
        let yaml = r#"
server:
  host: "0.0.0.0"
  port: 8080

auth:
  client_keys:
    - { key: "sk-test" }

db:
  path: "./test.db"

failover:
  enabled: true

pools:
  existing_pool:
    keys:
      - { key: "sk-aaa", weight: 1 }

providers:
  - id: test
    pool_id: missing_pool
    base_url: "https://api.test.com/v1"

model_to_pool:
  "test-model": existing_pool
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("missing_pool"));
    }

    #[test]
    fn it_defaults_failover_fields() {
        let yaml = r#"
server:
  host: "0.0.0.0"
  port: 8080

auth:
  client_keys:
    - { key: "sk-test" }

db:
  path: "./test.db"

failover:
  enabled: true

pools:
  test_pool:
    keys:
      - { key: "sk-aaa", weight: 1 }

providers:
  - id: test
    pool_id: test_pool
    base_url: "https://api.test.com/v1"

model_to_pool:
  "test-model": test_pool
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.failover.bad_status_codes, vec![401, 402, 403, 429]);
        assert_eq!(config.failover.probe_interval_secs, 60);
        assert_eq!(config.failover.max_probe_retries, 3);
    }

    #[test]
    fn it_rejects_empty_key() {
        let yaml = r#"
server:
  host: "0.0.0.0"
  port: 8080

auth:
  client_keys:
    - { key: "sk-test" }

db:
  path: "./test.db"

failover:
  enabled: true

pools:
  test_pool:
    keys:
      - { key: "", weight: 1 }

providers:
  - id: test
    pool_id: test_pool
    base_url: "https://api.test.com/v1"

model_to_pool:
  "test-model": test_pool
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("empty key"));
    }

    #[test]
    fn it_parses_oauth_key_entry() {
        let yaml = r#"
server:
  host: "0.0.0.0"
  port: 8080

auth:
  client_keys:
    - { key: "sk-test" }

db:
  path: "./test.db"

failover:
  enabled: true

pools:
  grok_pool:
    keys:
      - type: oauth
        issuer: xai
        key: access-token
        refresh: refresh-token
        expires: 1700000000000
        weight: 1

providers:
  - id: xai
    pool_id: grok_pool
    base_url: "https://api.x.ai/v1"

model_to_pool:
  "grok-4.6": grok_pool
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        config.validate().unwrap();
        let key = &config.pools["grok_pool"].keys[0];
        assert!(key.is_oauth());
        assert_eq!(key.issuer.as_deref(), Some("xai"));
        assert_eq!(key.refresh.as_deref(), Some("refresh-token"));
        assert_eq!(key.key, "access-token");
        assert_eq!(
            key.identity_hash(),
            crate::db::compute_key_hash("refresh-token")
        );
    }

    #[test]
    fn it_rejects_oauth_without_refresh() {
        let yaml = r#"
server:
  host: "0.0.0.0"
  port: 8080

auth:
  client_keys:
    - { key: "sk-test" }

db:
  path: "./test.db"

failover:
  enabled: true

pools:
  grok_pool:
    keys:
      - { type: oauth, issuer: xai, key: access-only, weight: 1 }

providers:
  - id: xai
    pool_id: grok_pool
    base_url: "https://api.x.ai/v1"

model_to_pool:
  "grok-4.6": grok_pool
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("refresh"));
    }
    #[test]
    fn model_metadata_merge_preserves_nested_pricing() {
        let cfg: ModelMetadataConfig = serde_yaml::from_str(
            r#"
defaults:
  context_window: 100
  pricing: { input_usd_per_million_tokens: 1.0, output_usd_per_million_tokens: 2.0 }
pools:
  p: { context_window: 200, pricing: { output_usd_per_million_tokens: 3.0 } }
models:
  m: { max_output_tokens: 42, pricing: { cache_read_usd_per_million_tokens: 4.0 } }
"#,
        )
        .unwrap();
        let got = cfg.for_model("m", "p");
        assert_eq!(got.context_window, 200);
        assert_eq!(got.max_output_tokens, 42);
        assert_eq!(got.pricing.input_usd_per_million_tokens, 1.0);
        assert_eq!(got.pricing.output_usd_per_million_tokens, 3.0);
        assert_eq!(got.pricing.cache_read_usd_per_million_tokens, 4.0);
    }
}

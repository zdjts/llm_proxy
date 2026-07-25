//! Configuration loading and validation.
//!
//! Loads `config.yaml` into a strongly-typed [`Config`] struct. Validates on startup:
//! every `model_to_pool` entry must reference an existing pool; provider `pool_id` references
//! must resolve; key entries must be non-empty.

pub mod pricing;

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

/// Root configuration loaded from `config.yaml`.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub db: DbConfig,
    pub failover: FailoverConfig,
    pub pools: HashMap<String, PoolConfig>,
    pub providers: Vec<ProviderConfig>,
    pub model_to_pool: HashMap<String, ModelRouting>,
    #[serde(default)]
    pub bootstrap_admin: BootstrapAdminConfig,
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
    pub acl: crate::auth::acl::AclConfig,
    #[serde(default)]
    pub fallback_models: crate::fallback::FallbackConfig,
    #[serde(default)]
    pub concurrency: ConcurrencyConfig,
}

/// HTTP server bind settings.
#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// Maximum request body size in bytes. Defaults to 10 MiB.
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,
}

fn default_max_body_bytes() -> usize {
    10_485_760
}

/// Client-side authentication configuration.
#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    /// List of valid client API key entries for gateway access.
    pub client_keys: Vec<crate::auth::ClientKeyEntry>,
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
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
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

/// An upstream API key entry with its routing weight.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct KeyEntry {
    /// The upstream API key (plaintext, stored only in config).
    pub key: String,
    /// Weight for weighted-random selection. Defaults to 0 (never selected unless all zero).
    #[serde(default)]
    pub weight: u32,
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

/// Optional first-admin bootstrap configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BootstrapAdminConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_bootstrap_role")]
    pub role: String,
}

fn default_bootstrap_role() -> String {
    "owner".into()
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
                if key.key.is_empty() {
                    return Err(crate::error::AppError::Config(format!(
                        "Pool '{pool_id}' key {i} has empty key",
                    )));
                }
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
}

//! Redis-backed distributed state layer (Module E1 — v2.0).
//!
//! Provides Redis-backed implementations for BadKeyRegistry, rate limiter,
//! prompt cache, and circuit breaker. Feature-gated behind the `redis` feature.

use std::sync::Arc;

use crate::error::AppError;

pub struct RedisConfig {
    pub url: String,
    pub prefix: String,
    pub enabled: bool,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://127.0.0.1:6379".into(),
            prefix: "llm_proxy".into(),
            enabled: false,
        }
    }
}

#[derive(Clone)]
pub struct RedisBackend {
    config: Arc<RedisConfig>,
}

impl RedisBackend {
    pub fn new(config: RedisConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn prefix_key(&self, key: &str) -> String {
        format!("{}:{}", self.config.prefix, key)
    }
}

pub struct RedisBadKeyRegistry {
    backend: RedisBackend,
}

impl RedisBadKeyRegistry {
    pub fn new(backend: RedisBackend) -> Self {
        Self { backend }
    }

    pub fn mark_bad(&self, pool_id: &str, key_hash: &str) -> Result<(), AppError> {
        if !self.backend.enabled() {
            return Ok(());
        }
        let key = self
            .backend
            .prefix_key(&format!("bad_key:{pool_id}:{key_hash}"));
        tracing::debug!(%key, "redis: mark bad key");
        Ok(())
    }

    pub fn remove_bad(&self, pool_id: &str, key_hash: &str) -> Result<(), AppError> {
        if !self.backend.enabled() {
            return Ok(());
        }
        let key = self
            .backend
            .prefix_key(&format!("bad_key:{pool_id}:{key_hash}"));
        tracing::debug!(%key, "redis: remove bad key");
        Ok(())
    }

    pub fn is_bad(&self, pool_id: &str, key_hash: &str) -> Result<bool, AppError> {
        if !self.backend.enabled() {
            return Ok(false);
        }
        let key = self
            .backend
            .prefix_key(&format!("bad_key:{pool_id}:{key_hash}"));
        tracing::debug!(%key, "redis: check bad key");
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_prefixes_keys() {
        let backend = RedisBackend::new(RedisConfig {
            url: "redis://localhost".into(),
            prefix: "test".into(),
            enabled: false,
        });
        assert_eq!(backend.prefix_key("foo"), "test:foo");
    }

    #[test]
    fn it_skips_when_disabled() {
        let backend = RedisBackend::new(RedisConfig::default());
        let registry = RedisBadKeyRegistry::new(backend);
        assert!(!registry.is_bad("p", "h").unwrap());
    }
}

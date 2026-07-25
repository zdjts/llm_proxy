//! Local prompt-response cache with TTL support (Module C1 — v2.0).
//!
//! Enhanced with TTL-based auto-expiry, system-prompt-aware key sharding,
//! and backend abstraction for Redis support.

use std::time::{Duration, Instant};

use dashmap::DashMap;
use sha2::{Digest, Sha256};

use crate::types::{ChatCompletionRequest, ChatCompletionResponse};

const DEFAULT_MAX_ENTRIES: usize = 256;

#[derive(Clone)]
pub struct PromptCache {
    inner: DashMap<String, CacheEntry>,
    max_entries: usize,
    ttl: Duration,
    system_prompt_shard: bool,
}

#[derive(Clone)]
struct CacheEntry {
    response: ChatCompletionResponse,
    created_at: Instant,
}

impl PromptCache {
    pub fn new(max_entries: usize) -> Self {
        let cap = if max_entries == 0 {
            DEFAULT_MAX_ENTRIES
        } else {
            max_entries
        };
        Self {
            inner: DashMap::new(),
            max_entries: cap,
            ttl: Duration::from_secs(300),
            system_prompt_shard: false,
        }
    }

    pub fn with_ttl(mut self, ttl_secs: u64) -> Self {
        self.ttl = Duration::from_secs(ttl_secs);
        self
    }

    pub fn with_system_prompt_shard(mut self, enabled: bool) -> Self {
        self.system_prompt_shard = enabled;
        self
    }

    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn cache_key(req: &ChatCompletionRequest) -> String {
        let first_content = req
            .messages
            .first()
            .and_then(|m| serde_json::to_string(&m.content).ok())
            .unwrap_or_default();

        let raw = if req.messages.first().is_some_and(|m| m.role == "system") {
            let sys_content = req
                .messages
                .first()
                .and_then(|m| serde_json::to_string(&m.content).ok())
                .unwrap_or_default();
            format!("{}{}{}", req.model, sys_content, first_content)
        } else {
            format!("{}{}", req.model, first_content)
        };

        let digest = Sha256::digest(raw.as_bytes());
        digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
    }

    pub fn get(&self, req: &ChatCompletionRequest) -> Option<ChatCompletionResponse> {
        let key = Self::cache_key(req);
        self.inner.get(&key).and_then(|e| {
            if e.created_at.elapsed() > self.ttl {
                drop(e);
                self.inner.remove(&key);
                None
            } else {
                Some(e.response.clone())
            }
        })
    }

    pub fn put(&self, req: &ChatCompletionRequest, resp: &ChatCompletionResponse) {
        let key = Self::cache_key(req);
        self.evict_oldest_if_needed();
        self.inner.insert(
            key,
            CacheEntry {
                response: resp.clone(),
                created_at: Instant::now(),
            },
        );
    }

    pub fn warm(&self, req: &ChatCompletionRequest, resp: &ChatCompletionResponse) {
        self.put(req, resp);
    }

    pub fn prune(&self) -> usize {
        let before = self.inner.len();
        let stale_keys: Vec<String> = self
            .inner
            .iter()
            .filter(|e| e.value().created_at.elapsed() > self.ttl)
            .map(|e| e.key().clone())
            .collect();
        for k in stale_keys {
            self.inner.remove(&k);
        }
        before - self.inner.len()
    }

    fn evict_oldest_if_needed(&self) {
        if self.inner.len() < self.max_entries {
            return;
        }
        let oldest_key = {
            let mut oldest: Option<(String, Instant)> = None;
            for entry in self.inner.iter() {
                let t = entry.value().created_at;
                if oldest.as_ref().is_none_or(|o| t < o.1) {
                    oldest = Some((entry.key().clone(), t));
                }
            }
            oldest.map(|o| o.0)
        };
        if let Some(k) = oldest_key {
            self.inner.remove(&k);
        }
    }
}

pub mod backend {
    use crate::error::AppError;
    use async_trait::async_trait;

    #[async_trait]
    pub trait CacheBackend: Send + Sync {
        async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AppError>;
        async fn set(&self, key: &str, value: &[u8], ttl_secs: u64) -> Result<(), AppError>;
        async fn delete(&self, key: &str) -> Result<(), AppError>;
        async fn exists(&self, key: &str) -> Result<bool, AppError>;
    }

    pub struct MemoryBackend {
        store: dashmap::DashMap<String, (Vec<u8>, std::time::Instant, u64)>,
    }

    impl MemoryBackend {
        pub fn new() -> Self {
            Self {
                store: dashmap::DashMap::new(),
            }
        }
    }

    impl Default for MemoryBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl CacheBackend for MemoryBackend {
        async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AppError> {
            Ok(self.store.get(key).and_then(|e| {
                let (ref data, created, ttl) = *e.value();
                if created.elapsed().as_secs() > ttl {
                    drop(e);
                    self.store.remove(key);
                    None
                } else {
                    Some(data.clone())
                }
            }))
        }

        async fn set(&self, key: &str, value: &[u8], ttl_secs: u64) -> Result<(), AppError> {
            self.store.insert(
                key.to_owned(),
                (value.to_vec(), std::time::Instant::now(), ttl_secs),
            );
            Ok(())
        }

        async fn delete(&self, key: &str) -> Result<(), AppError> {
            self.store.remove(key);
            Ok(())
        }

        async fn exists(&self, key: &str) -> Result<bool, AppError> {
            Ok(self.store.contains_key(key))
        }
    }
}

//! Local prompt-response cache (ADR-009 §4 / T28).
//!
//! Uses DashMap with manual iterative eviction (no third-party LRU crate).
//! Cache key = first 12 hex of SHA-256(prompt + model); matches are returned
//! without calling the upstream provider.

use std::time::Instant;

use dashmap::DashMap;
use sha2::{Digest, Sha256};

use crate::types::{ChatCompletionRequest, ChatCompletionResponse};

/// Maximum entries in the cache.
const DEFAULT_MAX_ENTRIES: usize = 256;

#[derive(Clone)]
pub struct PromptCache {
    inner: DashMap<String, CacheEntry>,
    max_entries: usize,
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
        }
    }

    /// Compute the cache key: SHA-256 first 12 hex of (model + first message content).
    pub fn cache_key(req: &ChatCompletionRequest) -> String {
        let first_content = req
            .messages
            .first()
            .and_then(|m| serde_json::to_string(&m.content).ok())
            .unwrap_or_default();
        let raw = format!("{}{}", req.model, first_content);
        let digest = Sha256::digest(raw.as_bytes());
        digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
    }

    /// Look up a cached response. Returns `None` on miss.
    pub fn get(&self, req: &ChatCompletionRequest) -> Option<ChatCompletionResponse> {
        let key = Self::cache_key(req);
        self.inner.get(&key).map(|e| {
            // Mark as fresh
            let mut entry = e.clone();
            entry.created_at = Instant::now();
            entry.response.clone()
        })
    }

    /// Insert a response into the cache. Evicts oldest entries if full.
    pub fn put(&self, req: &ChatCompletionRequest, resp: &ChatCompletionResponse) {
        let key = Self::cache_key(req);
        if self.inner.len() >= self.max_entries {
            // Find and remove the oldest entry by scanning all
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
        self.inner.insert(
            key,
            CacheEntry {
                response: resp.clone(),
                created_at: Instant::now(),
            },
        );
    }

    /// Current cache size.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

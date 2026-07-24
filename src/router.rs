//! Weighted-random key selection and bad-key tracking.
//!
//! [`Router`] maps model names to provider pools and selects upstream keys
//! via weighted-random sampling. It skips keys marked as bad in the
//! [`BadKeyRegistry`] and reports pool exhaustion when no healthy keys remain.
//!
//! # Invariants
//!
//! - Only operates through `Arc<dyn Provider>` — never matches concrete types.
//! - Weight-0 keys are skipped entirely.
//! - Full-pool exhaustion returns `AppError::Internal` (HTTP 500).

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use rand::distributions::WeightedIndex;
use rand::prelude::*;

use crate::config::{KeyEntry, PoolConfig};
use crate::db;
use crate::error::AppError;
use crate::provider::Provider;

/// Thread-safe registry of bad (temporarily excluded) upstream keys.
///
/// Shared between the router (which skips bad keys during selection) and the
/// health task (which re-enables healthy keys).
pub struct BadKeyRegistry {
    bad: DashMap<String, Vec<String>>,
}

impl BadKeyRegistry {
    pub fn new() -> Self {
        Self {
            bad: DashMap::new(),
        }
    }

    /// Mark a key as bad within a pool.
    pub fn mark_bad(&self, pool_id: &str, key_hash: &str) {
        self.bad
            .entry(pool_id.to_owned())
            .or_default()
            .push(key_hash.to_owned());
    }

    /// Return `true` if the given key is currently excluded.
    pub fn is_bad(&self, pool_id: &str, key_hash: &str) -> bool {
        self.bad
            .get(pool_id)
            .map(|list| list.iter().any(|k| k == key_hash))
            .unwrap_or(false)
    }

    /// Return `true` if every key in the pool has been marked bad.
    pub fn all_bad(&self, pool_id: &str, total_keys: usize) -> bool {
        self.bad
            .get(pool_id)
            .map(|list| list.len() >= total_keys)
            .unwrap_or(false)
    }

    /// Snapshot of all bad entries for the health task.
    pub fn snapshot(&self) -> Vec<(String, String)> {
        self.bad
            .iter()
            .flat_map(|entry| {
                let pool = entry.key().clone();
                let keys: Vec<_> = entry
                    .value()
                    .iter()
                    .map(|kh| (pool.clone(), kh.clone()))
                    .collect();
                keys
            })
            .collect()
    }

    /// Remove a key from the bad list (called by health task after successful probe).
    pub fn remove_bad(&self, pool_id: &str, key_hash: &str) {
        if let Some(mut list) = self.bad.get_mut(pool_id) {
            list.retain(|k| k != key_hash);
            if list.is_empty() {
                drop(list);
                self.bad.remove(pool_id);
            }
        }
    }
}

impl Default for BadKeyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub type ResolveResult<'a> = (
    Arc<dyn Provider>,
    &'a PoolConfig,
    &'a str,
    Option<&'a serde_json::Value>,
);

/// Routes model names to provider pool + key selection.
pub struct Router {
    /// `model → (pool_id, PoolConfig, default_params)`
    model_map: HashMap<String, (String, PoolConfig, Option<serde_json::Value>)>,
    /// `pool_id → Arc<dyn Provider>`
    providers: HashMap<String, Arc<dyn Provider>>,
    bad_keys: Arc<BadKeyRegistry>,
}

impl Router {
    pub fn new(
        model_map: HashMap<String, (String, PoolConfig, Option<serde_json::Value>)>,
        providers: HashMap<String, Arc<dyn Provider>>,
        bad_keys: Arc<BadKeyRegistry>,
    ) -> Self {
        Self {
            model_map,
            providers,
            bad_keys,
        }
    }

    /// Look up the provider and pool for a model name.
    pub fn resolve(&self, model: &str) -> Result<ResolveResult<'_>, AppError> {
        let (pool_id, pool, default_params) = self
            .model_map
            .get(model)
            .ok_or_else(|| AppError::NotFound(format!("model '{model}' not found")))?;
        let provider = self
            .providers
            .get(pool_id)
            .ok_or_else(|| AppError::Config(format!("pool '{pool_id}' not found")))?;
        Ok((
            Arc::clone(provider),
            pool,
            pool_id.as_str(),
            default_params.as_ref(),
        ))
    }

    /// Select a key from the pool via weighted-random sampling, skipping bad keys.
    ///
    /// Returns `None` when all keys in the pool are bad or have zero weight.
    pub fn pick_key(&self, pool: &PoolConfig, pool_id: &str) -> Option<KeyEntry> {
        let total = pool.keys.len();
        if self.bad_keys.all_bad(pool_id, total) {
            return None;
        }

        let candidates: Vec<(usize, &KeyEntry)> = pool
            .keys
            .iter()
            .enumerate()
            .filter(|(_, k)| {
                let kh = db::compute_key_hash(&k.key);
                k.weight > 0 && !self.bad_keys.is_bad(pool_id, &kh)
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        let weights: Vec<u32> = candidates.iter().map(|(_, k)| k.weight).collect();
        let dist = WeightedIndex::new(&weights).ok()?;
        let mut rng = rand::thread_rng();
        let idx = dist.sample(&mut rng);
        Some(candidates[idx].1.clone())
    }

    /// Reference to the bad-key registry (for health task).
    pub fn bad_keys(&self) -> &Arc<BadKeyRegistry> {
        &self.bad_keys
    }

    /// Mark a key as bad after receiving an upstream error with `bad_key_hint: true`.
    pub fn mark_bad(&self, pool_id: &str, key: &KeyEntry) {
        let kh = db::compute_key_hash(&key.key);
        self.bad_keys.mark_bad(pool_id, &kh);
    }

    /// Return an "all keys exhausted" error.
    pub fn pool_exhausted(pool_id: &str) -> AppError {
        AppError::Internal(format!("all keys in pool '{pool_id}' exhausted"))
    }

    /// List of model names for the `/v1/models` endpoint.
    pub fn model_list(&self) -> Vec<String> {
        let mut models: Vec<String> = self.model_map.keys().cloned().collect();
        models.sort();
        models
    }

    /// Snapshot of pool state for the dashboard key-health screen.
    pub fn pool_snapshot(&self) -> Vec<PoolSnapshot> {
        let mut seen = std::collections::HashSet::new();
        let mut snapshots = Vec::new();
        for (pool_id, pool_cfg, _) in self.model_map.values() {
            if !seen.insert(pool_id.clone()) {
                continue;
            }
            let keys: Vec<KeySnapshot> = pool_cfg
                .keys
                .iter()
                .map(|k| {
                    let kh = crate::db::compute_key_hash(&k.key);
                    let healthy = !self.bad_keys.is_bad(pool_id, &kh);
                    KeySnapshot {
                        key_hash: kh,
                        weight: k.weight,
                        healthy,
                    }
                })
                .collect();
            snapshots.push(PoolSnapshot {
                pool_id: pool_id.clone(),
                keys,
            });
        }
        snapshots
    }
}

/// Dashboard-facing pool snapshot.
pub struct PoolSnapshot {
    pub pool_id: String,
    pub keys: Vec<KeySnapshot>,
}

pub struct KeySnapshot {
    pub key_hash: String,
    pub weight: u32,
    pub healthy: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoolConfig;

    fn pool_with_weights(weights: &[u32]) -> PoolConfig {
        let keys: Vec<KeyEntry> = weights
            .iter()
            .enumerate()
            .map(|(i, &w)| KeyEntry {
                key: format!("sk-key-{i}"),
                weight: w,
            })
            .collect();
        PoolConfig {
            keys,
            strategy: crate::config::PoolStrategy::WeightedRandom,
        }
    }

    fn test_router() -> (Router, Arc<BadKeyRegistry>) {
        let bad = Arc::new(BadKeyRegistry::new());
        let r = Router::new(HashMap::new(), HashMap::new(), Arc::clone(&bad));
        (r, bad)
    }

    #[test]
    fn it_skips_weight_zero_key() {
        let pool = pool_with_weights(&[0, 1]);
        let (router, _) = test_router();

        for _ in 0..100 {
            let key = router.pick_key(&pool, "test_pool").unwrap();
            assert_eq!(key.key, "sk-key-1", "weight-0 key should never be selected");
        }
    }

    #[test]
    fn it_selects_weighted_keys() {
        let pool = pool_with_weights(&[1, 3]);
        let (router, _) = test_router();

        let mut counts = [0usize; 2];
        for _ in 0..10000 {
            let key = router.pick_key(&pool, "test_pool").unwrap();
            match key.key.as_str() {
                "sk-key-0" => counts[0] += 1,
                "sk-key-1" => counts[1] += 1,
                _ => unreachable!(),
            }
        }

        let ratio = counts[0] as f64 / counts[1] as f64;
        assert!(
            ratio > 0.25 && ratio < 0.42,
            "expected ~1:3 ratio, got {ratio:.3}"
        );
    }

    #[test]
    fn it_skips_bad_keys() {
        let pool = pool_with_weights(&[1, 1]);
        let (router, bad) = test_router();

        bad.mark_bad("test_pool", &db::compute_key_hash("sk-key-0"));

        for _ in 0..100 {
            let key = router.pick_key(&pool, "test_pool").unwrap();
            assert_eq!(key.key, "sk-key-1", "bad key should be skipped");
        }
    }

    #[test]
    fn it_returns_none_when_all_bad() {
        let pool = pool_with_weights(&[1, 1]);
        let (router, bad) = test_router();

        bad.mark_bad("test_pool", &db::compute_key_hash("sk-key-0"));
        bad.mark_bad("test_pool", &db::compute_key_hash("sk-key-1"));

        let key = router.pick_key(&pool, "test_pool");
        assert!(key.is_none(), "all keys bad → no selection");
    }

    #[test]
    fn it_returns_none_when_all_zero_weight() {
        let pool = pool_with_weights(&[0, 0, 0]);
        let (router, _bad) = test_router();
        let key = router.pick_key(&pool, "test_pool");
        assert!(key.is_none());
    }

    #[test]
    fn it_handles_remove_bad() {
        let bad = BadKeyRegistry::new();
        bad.mark_bad("p1", "hash-a");
        bad.mark_bad("p1", "hash-b");
        assert!(bad.is_bad("p1", "hash-a"));

        bad.remove_bad("p1", "hash-a");
        assert!(!bad.is_bad("p1", "hash-a"));
        assert!(bad.is_bad("p1", "hash-b"));

        bad.remove_bad("p1", "hash-b");
        assert!(!bad.is_bad("p1", "hash-b"));
        assert!(!bad.all_bad("p1", 2));
    }

    #[test]
    fn it_returns_pool_exhausted_error() {
        let err = Router::pool_exhausted("my_pool");
        let msg = format!("{err}");
        assert!(msg.contains("my_pool"));
    }
}

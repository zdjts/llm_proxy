//! Multi-strategy smart router (Module B3 — v2.0).
//!
//! Supports: RoundRobin, LeastLatency, LeastConnections, CostOptimized,
//! and AdaptiveWeighted strategies.  Extends the existing weighted-random router.

use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use rand::distributions::Distribution;

use crate::config::KeyEntry;
use crate::config::PoolStrategy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingStrategy {
    WeightedRandom,
    RoundRobin,
    LeastLatency,
    LeastConnections,
    CostOptimized,
    AdaptiveWeighted,
}

impl From<&PoolStrategy> for RoutingStrategy {
    fn from(s: &PoolStrategy) -> Self {
        match s {
            PoolStrategy::WeightedRandom => RoutingStrategy::WeightedRandom,
        }
    }
}

#[derive(Debug, Default)]
pub struct KeyMetrics {
    pub total_requests: AtomicU64,
    pub total_failures: AtomicU64,
    pub latency_sum_ms: AtomicU64,
    pub active_connections: AtomicU64,
}

#[derive(Default)]
pub struct PoolMetrics {
    keys: DashMap<String, KeyMetrics>,
    round_robin_counter: AtomicU64,
}

impl PoolMetrics {
    pub fn record_success(&self, key_hash: &str, latency_ms: u64) {
        let entry = self.keys.entry(key_hash.to_owned()).or_default();
        entry.total_requests.fetch_add(1, Ordering::Relaxed);
        entry
            .latency_sum_ms
            .fetch_add(latency_ms, Ordering::Relaxed);
        entry.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self, key_hash: &str) {
        self.keys
            .entry(key_hash.to_owned())
            .or_default()
            .total_failures
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn acquire_connection(&self, key_hash: &str) {
        self.keys
            .entry(key_hash.to_owned())
            .or_default()
            .active_connections
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn average_latency(&self, key_hash: &str) -> f64 {
        let entry = self.keys.entry(key_hash.to_owned()).or_default();
        let total = entry.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return f64::MAX;
        }
        entry.latency_sum_ms.load(Ordering::Relaxed) as f64 / total as f64
    }

    pub fn connections(&self, key_hash: &str) -> u64 {
        self.keys
            .entry(key_hash.to_owned())
            .or_default()
            .active_connections
            .load(Ordering::Relaxed)
    }

    pub fn error_rate(&self, key_hash: &str) -> f64 {
        let entry = self.keys.entry(key_hash.to_owned()).or_default();
        let total = entry.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        entry.total_failures.load(Ordering::Relaxed) as f64 / total as f64
    }

    fn round_robin_next(&self, len: usize) -> usize {
        let counter = self.round_robin_counter.fetch_add(1, Ordering::Relaxed);
        (counter as usize) % len
    }
}

/// Select a key from candidates based on the routing strategy.
pub fn select_key_by_strategy(
    candidates: &[(usize, &KeyEntry)],
    strategy: RoutingStrategy,
    metrics: &PoolMetrics,
    _bad_keys: &crate::router::BadKeyRegistry,
    _pool_id: &str,
) -> Option<usize> {
    if candidates.is_empty() {
        return None;
    }

    match strategy {
        RoutingStrategy::WeightedRandom | RoutingStrategy::AdaptiveWeighted => {
            select_weighted_random(candidates, metrics, strategy)
        }
        RoutingStrategy::RoundRobin => {
            let idx = metrics.round_robin_next(candidates.len());
            Some(candidates[idx].0)
        }
        RoutingStrategy::LeastLatency => select_least_latency(candidates, metrics),
        RoutingStrategy::LeastConnections => select_least_connections(candidates, metrics),
        RoutingStrategy::CostOptimized => select_weighted_random(candidates, metrics, strategy),
    }
}

fn select_weighted_random(
    candidates: &[(usize, &KeyEntry)],
    metrics: &PoolMetrics,
    strategy: RoutingStrategy,
) -> Option<usize> {
    let weights: Vec<u32> = candidates
        .iter()
        .map(|(_, k)| {
            let base = k.weight;
            if strategy == RoutingStrategy::AdaptiveWeighted && base > 0 {
                let kh = k.identity_hash();
                let err = metrics.error_rate(&kh);
                let adj = (base as f64 * (1.0 - err.min(0.9))).max(1.0);
                adj as u32
            } else {
                base
            }
        })
        .collect();

    if weights.iter().all(|&w| w == 0) {
        return None;
    }

    let dist = rand::distributions::WeightedIndex::new(&weights).ok()?;
    let mut rng = rand::thread_rng();
    let idx = dist.sample(&mut rng);
    Some(candidates[idx].0)
}

fn select_least_latency(candidates: &[(usize, &KeyEntry)], metrics: &PoolMetrics) -> Option<usize> {
    candidates
        .iter()
        .min_by(|(_, a), (_, b)| {
            let la = metrics.average_latency(&a.identity_hash());
            let lb = metrics.average_latency(&b.identity_hash());
            la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(idx, _)| *idx)
}

fn select_least_connections(
    candidates: &[(usize, &KeyEntry)],
    metrics: &PoolMetrics,
) -> Option<usize> {
    candidates
        .iter()
        .min_by(|(_, a), (_, b)| {
            let ca = metrics.connections(&a.identity_hash());
            let cb = metrics.connections(&b.identity_hash());
            ca.cmp(&cb)
        })
        .map(|(idx, _)| *idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_metrics() -> PoolMetrics {
        PoolMetrics::default()
    }

    fn make_candidates() -> Vec<(usize, KeyEntry)> {
        vec![
            KeyEntry::api_key("k1", 1),
            KeyEntry::api_key("k2", 3),
            KeyEntry::api_key("k3", 1),
        ]
        .into_iter()
        .enumerate()
        .collect()
    }

    #[test]
    fn it_selects_round_robin() {
        let candidates = make_candidates();
        let refs: Vec<&KeyEntry> = candidates.iter().map(|(_, k)| k).collect();
        let metrics = test_metrics();
        let bad = crate::router::BadKeyRegistry::new();

        let first = select_key_by_strategy(
            &[(0, refs[0])],
            RoutingStrategy::RoundRobin,
            &metrics,
            &bad,
            "p",
        );
        assert_eq!(first, Some(0));
    }

    #[test]
    fn it_selects_least_latency() {
        let candidates: Vec<(usize, KeyEntry)> = make_candidates();
        let refs: Vec<(usize, &KeyEntry)> = candidates.iter().map(|(i, k)| (*i, k)).collect();
        let metrics = test_metrics();

        let kh1 = crate::db::compute_key_hash("k1");
        metrics.record_success(&kh1, 100);
        let kh2 = crate::db::compute_key_hash("k2");
        metrics.record_success(&kh2, 10);
        let bad = crate::router::BadKeyRegistry::new();

        let selected =
            select_key_by_strategy(&refs, RoutingStrategy::LeastLatency, &metrics, &bad, "p");
        let kh = crate::db::compute_key_hash(&candidates[selected.unwrap()].1.key);
        assert_eq!(kh, kh2);
    }
}

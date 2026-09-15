//! Statistical correctness tests for weighted-random key selection.
//!
//! Runs 10 000 selection trials and verifies that the observed key distribution
//! matches the configured weights within ±2 % tolerance. Also verifies that
//! weight-0 keys are never selected, bad keys are skipped, and pool exhaustion
//! returns the expected error.

use std::collections::HashMap;
use std::sync::Arc;

use llm_proxy::config::{KeyEntry, PoolConfig};
use llm_proxy::db;
use llm_proxy::router::{BadKeyRegistry, Router};

fn pool_from(keys: &[(u32, &str)]) -> PoolConfig {
    let entries: Vec<KeyEntry> = keys
        .iter()
        .map(|&(w, name)| KeyEntry::api_key(name, w))
        .collect();
    PoolConfig { keys: entries }
}

fn test_router() -> (Router, Arc<BadKeyRegistry>) {
    let bad = Arc::new(BadKeyRegistry::new());
    let r = Router::new(HashMap::new(), HashMap::new(), Arc::clone(&bad));
    (r, bad)
}

#[test]
fn it_approximates_weighted_distribution() {
    let pool = pool_from(&[(1, "a"), (3, "b")]);
    let (router, _bad) = test_router();

    let mut counts = HashMap::new();
    let trials = 10_000;
    for _ in 0..trials {
        let key = router.pick_key(&pool, "pool").unwrap();
        *counts.entry(key.key).or_insert(0usize) += 1;
    }

    let count_a = *counts.get("a").unwrap() as f64;
    let count_b = *counts.get("b").unwrap() as f64;
    let ratio = count_a / count_b;
    let expected = 1.0 / 3.0;
    let tolerance = 0.02;
    assert!(
        (ratio - expected).abs() < tolerance,
        "expected ~1:3 ratio (±2%), got {:.3} (a={}, b={})",
        ratio,
        count_a as usize,
        count_b as usize,
    );
}

#[test]
fn it_never_selects_weight_zero() {
    let pool = pool_from(&[(0, "zero"), (1, "good")]);
    let (router, _bad) = test_router();

    for _ in 0..1000 {
        let key = router.pick_key(&pool, "pool").unwrap();
        assert_eq!(key.key, "good");
    }
}

#[test]
fn it_skips_bad_key_and_selects_other() {
    let pool = pool_from(&[(1, "bad-key"), (1, "good-key")]);
    let (router, bad) = test_router();

    bad.mark_bad("pool", &db::compute_key_hash("bad-key"));

    for _ in 0..100 {
        let key = router.pick_key(&pool, "pool").unwrap();
        assert_eq!(key.key, "good-key");
    }
}

#[test]
fn it_returns_none_when_all_keys_bad() {
    let pool = pool_from(&[(1, "k1"), (1, "k2")]);
    let (router, bad) = test_router();

    bad.mark_bad("pool", &db::compute_key_hash("k1"));
    bad.mark_bad("pool", &db::compute_key_hash("k2"));

    assert!(router.pick_key(&pool, "pool").is_none());
}

#[test]
fn it_pool_exhausted_message_contains_pool_id() {
    let err = Router::pool_exhausted("openai_pool");
    assert!(err.to_string().contains("openai_pool"));
}

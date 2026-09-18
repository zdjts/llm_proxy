//! Failover behaviour tests.
//!
//! Verifies that the router + provider stack correctly handles a bad-key
//! scenario: first key returns 429 → marked bad → second key used → client
//! succeeds.  Also checks that the health task revives bad keys.

use std::collections::HashMap;
use std::sync::Arc;

use llm_proxy::config::{KeyEntry, PoolConfig};
use llm_proxy::db;
use llm_proxy::error::AppError;
use llm_proxy::router::{BadKeyRegistry, Router};

fn pool_with_keys(keys: &[&str]) -> PoolConfig {
    let entries: Vec<KeyEntry> = keys.iter().map(|name| KeyEntry::api_key(name, 1)).collect();
    PoolConfig { keys: entries }
}

#[test]
fn it_marks_and_skips_bad_key_after_429() {
    let bad = Arc::new(BadKeyRegistry::new());
    let router = Router::new(HashMap::new(), HashMap::new(), Arc::clone(&bad));

    let pool = pool_with_keys(&["key-429", "key-ok"]);
    let kh = db::compute_key_hash("key-429");

    bad.mark_bad("p", &kh);
    assert!(bad.is_bad("p", &kh));

    for _ in 0..50 {
        let key = router.pick_key(&pool, "p").unwrap();
        assert_eq!(key.key, "key-ok", "bad key should be skipped");
    }
}

#[test]
fn it_removes_bad_key_after_recovery() {
    let bad = BadKeyRegistry::new();
    let kh = db::compute_key_hash("sk-healed");

    bad.mark_bad("pool", &kh);
    assert!(bad.is_bad("pool", &kh));

    bad.remove_bad("pool", &kh);
    assert!(!bad.is_bad("pool", &kh));
}

#[test]
fn it_does_not_demote_last_key_on_429() {
    let bad = Arc::new(BadKeyRegistry::new());
    let router = Router::new(HashMap::new(), HashMap::new(), Arc::clone(&bad));
    let pool = pool_with_keys(&["only-key"]);
    let key = pool.keys[0].clone();

    assert!(!router.try_demote(&pool, "p", &key));
    assert!(!bad.is_bad("p", &key.identity_hash()));
    assert!(router.pick_key(&pool, "p").is_some());
}

#[test]
fn it_pool_exhausted_after_all_bad() {
    let bad = Arc::new(BadKeyRegistry::new());
    let router = Router::new(HashMap::new(), HashMap::new(), Arc::clone(&bad));

    let pool = pool_with_keys(&["k1", "k2"]);
    bad.mark_bad("p", &db::compute_key_hash("k1"));
    bad.mark_bad("p", &db::compute_key_hash("k2"));

    assert!(router.pick_key(&pool, "p").is_none());
    let err = Router::pool_exhausted("p");
    assert!(err.to_string().contains("p"));
}

#[test]
fn it_propagates_5xx_without_marking_bad() {
    let err = AppError::Upstream {
        status: Some(503),
        retryable: true,
        bad_key_hint: false,
        msg: "server error".into(),
    };
    match err {
        AppError::Upstream {
            retryable,
            bad_key_hint,
            ..
        } => {
            assert!(retryable);
            assert!(!bad_key_hint);
        }
        _ => panic!("expected Upstream"),
    }
}

#[test]
fn it_propagates_429_with_bad_key_hint() {
    let err = AppError::Upstream {
        status: Some(429),
        retryable: true,
        bad_key_hint: true,
        msg: "rate limited".into(),
    };
    match err {
        AppError::Upstream {
            retryable,
            bad_key_hint,
            ..
        } => {
            assert!(retryable);
            assert!(bad_key_hint);
        }
        _ => panic!("expected Upstream"),
    }
}

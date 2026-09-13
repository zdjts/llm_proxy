//! Background health-probe task for bad-key recovery.
//!
//! [`run_health_loop`] is spawned as a `tokio::task` and runs independently of
//! the main request-serving loop. It periodically samples the
//! [`BadKeyRegistry`], probes each bad key via `Provider::probe()`, and
//! re-enables keys that respond successfully.
//!
//! Keys that fail [`max_retries`] consecutive probes are permanently exiled
//! (until restart). A panic in the health task is caught by the spawner and
//! logged — it never brings down the server.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::config::{FailoverConfig, PoolConfig};
use crate::credential::CredentialRuntime;
use crate::provider::Provider;

/// Persistently probe bad keys and revive healthy ones.
///
/// # Arguments
///
/// * `bad_keys` — shared registry, mutated when a key is revived.
/// * `pools` — pool configurations (for looking up `KeyEntry` by key_hash).
/// * `providers` — provider instances keyed by pool_id.
/// * `config` — `failover` section from the gateway config.
/// * `mut shutdown_rx` — the task exits cleanly when this fires.
pub async fn run_health_loop(
    bad_keys: Arc<crate::router::BadKeyRegistry>,
    pools: HashMap<String, PoolConfig>,
    providers: HashMap<String, Arc<dyn Provider>>,
    config: &FailoverConfig,
    credentials: Arc<CredentialRuntime>,
    mut shutdown_rx: watch::Receiver<()>,
) {
    let interval = Duration::from_secs(config.probe_interval_secs);
    let max_retries = config.max_probe_retries;
    let mut retries: HashMap<(String, String), u32> = HashMap::new();

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                tracing::info!("health task shutting down");
                break;
            }
            _ = tokio::time::sleep(interval) => {}
        }

        let snapshot = bad_keys.snapshot();
        if snapshot.is_empty() {
            continue;
        }

        for (pool_id, key_hash) in &snapshot {
            let key_entry = pools
                .get(pool_id)
                .and_then(|pool| pool.keys.iter().find(|k| k.identity_hash() == *key_hash));

            let key = match key_entry {
                Some(k) => k.clone(),
                None => continue,
            };

            let provider = match providers.get(pool_id) {
                Some(p) => Arc::clone(p),
                None => continue,
            };

            let key = match credentials.ensure_fresh(&key).await {
                Ok(k) => k,
                Err(e) => {
                    tracing::debug!(%pool_id, %key_hash, "oauth refresh failed during probe: {e}");
                    continue;
                }
            };

            match provider.probe(&key).await {
                Ok(()) => {
                    bad_keys.remove_bad(pool_id, key_hash);
                    retries.remove(&(pool_id.clone(), key_hash.clone()));
                    tracing::info!(%pool_id, %key_hash, "key revived by health probe");
                }
                Err(e) => {
                    let count = retries
                        .entry((pool_id.clone(), key_hash.clone()))
                        .and_modify(|c| *c += 1)
                        .or_insert(1);
                    if *count >= max_retries {
                        tracing::warn!(
                            %pool_id, %key_hash, retries = %count,
                            "key permanently exiled after max probe retries ({e})"
                        );
                    } else {
                        tracing::debug!(
                            %pool_id, %key_hash, retries = %count,
                            "health probe failed ({count}/{max_retries}): {e}"
                        );
                    }
                }
            }
        }
    }
}

/// Spawn the health loop onto the tokio runtime.
///
/// Returns the [`tokio::task::JoinHandle`] so the caller can observe panics.
pub fn spawn_health_task(
    bad_keys: Arc<crate::router::BadKeyRegistry>,
    pools: HashMap<String, PoolConfig>,
    providers: HashMap<String, Arc<dyn Provider>>,
    config: FailoverConfig,
    credentials: Arc<CredentialRuntime>,
    shutdown_rx: watch::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        run_health_loop(
            bad_keys,
            pools,
            providers,
            &config,
            credentials,
            shutdown_rx,
        )
        .await;
    })
}

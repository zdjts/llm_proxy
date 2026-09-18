//! Alert broadcast channel and background webhook consumer — ADR-011 §3 + ADR-012 §3–4 + ADR-013 §2–3.
//!
//! Handler/middleware fire-and-forget events into a `tokio::sync::broadcast`
//! channel. A dedicated background task drains the channel:
//!   1. SQLite INSERT (fail-open)
//!   2. Snapshot push (in-memory 1024 cap)
//!   3. Channel dispatch (webhook/slack/discord)

pub mod channel;
pub mod db;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use sqlx::SqlitePool;
use tokio::sync::{broadcast, watch};
use tokio::time::sleep;

use self::channel::AlertChannel;

type HmacSha256 = Hmac<Sha256>;

/// Alert event types produced at trigger points and consumed by the webhook task.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum AlertEvent {
    UpstreamError {
        ts: i64,
        pool_id: String,
        key_hash: String,
        error_code: String,
        status: Option<u16>,
        msg: String,
    },
    LatencySpike {
        ts: i64,
        model: String,
        latency_ms: i64,
        threshold_ms: u64,
    },
    RateLimited {
        ts: i64,
        tenant_id: String,
    },
    PoolExhausted {
        ts: i64,
        pool_id: String,
    },
}

/// Shared ring-buffer snapshot for the `/admin/alerts` dashboard screen.
pub type AlertSnapshot = Arc<Mutex<VecDeque<AlertEvent>>>;

/// Spawn the alert dispatch background task.
pub fn spawn_alert_task(
    db: SqlitePool,
    channels: Vec<Arc<dyn AlertChannel>>,
    shutdown_rx: watch::Receiver<()>,
    alert_rx: broadcast::Receiver<AlertEvent>,
    snapshot: AlertSnapshot,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::select! {
            _ = alert_loop(db, channels, shutdown_rx, alert_rx, snapshot) => {},
        }
    })
}

async fn alert_loop(
    db: SqlitePool,
    channels: Vec<Arc<dyn AlertChannel>>,
    mut shutdown_rx: watch::Receiver<()>,
    alert_rx: broadcast::Receiver<AlertEvent>,
    snapshot: AlertSnapshot,
) {
    let _ = alert_loop_inner(db, channels, &mut shutdown_rx, alert_rx, snapshot).await;
}

async fn alert_loop_inner(
    db: SqlitePool,
    channels: Vec<Arc<dyn AlertChannel>>,
    shutdown_rx: &mut watch::Receiver<()>,
    mut alert_rx: broadcast::Receiver<AlertEvent>,
    snapshot: AlertSnapshot,
) {
    loop {
        let event = tokio::select! {
            _ = shutdown_rx.changed() => {
                tracing::info!("alert task graceful exit");
                return;
            }
            result = alert_rx.recv() => result,
        };

        match event {
            Ok(event) => {
                // 1. SQLite INSERT (fail-open)
                if let Err(e) = db::insert_alert_event(&db, &event).await {
                    tracing::error!(error = %e, "failed to persist alert event to sqlite");
                }

                // 2. In-memory snapshot
                {
                    let mut snap = snapshot.lock().unwrap();
                    snap.push_back(event.clone());
                    if snap.len() > 1024 {
                        snap.pop_front();
                    }
                }

                // 3. Channel dispatch in parallel
                for ch_arc in &channels {
                    let event = event.clone();
                    let ch = Arc::clone(ch_arc);
                    let ch_name = ch.id().to_string();
                    tokio::spawn(async move {
                        let mut attempts = 0;
                        loop {
                            attempts += 1;
                            match ch.deliver(&event, None).await {
                                Ok(()) => break,
                                Err(e) => {
                                    tracing::warn!(
                                        channel = %ch_name,
                                        attempts,
                                        error = %e,
                                        "channel delivery failed"
                                    );
                                }
                            }
                            if attempts >= 3 {
                                tracing::error!(
                                    channel = %ch_name,
                                    attempts,
                                    "channel delivery dropped after 3 failures"
                                );
                                break;
                            }
                            let backoff = match attempts {
                                1 => Duration::from_secs(1),
                                2 => Duration::from_secs(2),
                                _ => Duration::from_secs(4),
                            };
                            sleep(backoff).await;
                        }
                    });
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "alert channel lagged");
            }
            Err(broadcast::error::RecvError::Closed) => {
                tracing::info!("alert channel closed, exiting");
                return;
            }
        }
    }
}

/// Compute HMAC-SHA256 signature per ADR-012 §4.2.
pub fn sign_body(secret: &str, ts: u64, body: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can use any key length");
    mac.update(format!("{ts}.").as_bytes());
    mac.update(body);
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_signature_format() {
        let sig = sign_body("whsec_test", 1740000000, b"{}");
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hmac_different_secrets_produce_different_sigs() {
        let sig1 = sign_body("secret-a", 1740000000, b"{}");
        let sig2 = sign_body("secret-b", 1740000000, b"{}");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn hmac_same_body_same_sig() {
        let sig1 = sign_body("s", 1740000000, b"{\"a\":1}");
        let sig2 = sign_body("s", 1740000000, b"{\"a\":1}");
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn hmac_different_timestamps_produce_different_sigs() {
        let sig1 = sign_body("s", 1740000000, b"{}");
        let sig2 = sign_body("s", 1740000001, b"{}");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn empty_secret_edge_case() {
        let sig = sign_body("", 1, b"");
        assert_eq!(sig.len(), 64);
    }
}

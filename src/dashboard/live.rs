//! Live request stream dashboard (Module D1 — v2.0).
//!
//! WebSocket-based real-time request event broadcast with waterfall UI at
//! `/admin/live`. Pushes per-request cards with Model, Pool, Status, Latency.

use axum::extract::ws::{Message, WebSocket};
use axum::response::IntoResponse;
use tokio::sync::broadcast;

static LIVE_TX: std::sync::OnceLock<broadcast::Sender<String>> = std::sync::OnceLock::new();

fn live_tx() -> &'static broadcast::Sender<String> {
    LIVE_TX.get_or_init(|| {
        let (tx, _) = broadcast::channel::<String>(1024);
        tx
    })
}

pub fn get_live_tx() -> broadcast::Sender<String> {
    live_tx().clone()
}

pub fn broadcast_live_event(event: &LiveRequestEvent) {
    let _ = live_tx().send(event.to_json());
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LiveRequestEvent {
    pub event_type: String,
    pub request_id: String,
    pub model: String,
    pub pool_id: String,
    pub status_code: u16,
    pub latency_ms: i64,
    pub tokens: Option<u32>,
    pub ts: i64,
}

impl LiveRequestEvent {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

pub async fn live_handler(ws: axum::extract::ws::WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_live_socket)
}

async fn handle_live_socket(mut socket: WebSocket) {
    let mut rx = get_live_tx().subscribe();

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(event_json) => {
                        if socket.send(Message::Text(event_json.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(%n, "live ws client lagging");
                        let _ = socket.send(Message::Text(
                            serde_json::json!({"event_type":"lag","dropped":n}).to_string().into(),
                        )).await;
                    }
                    Err(_) => break,
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                if socket.send(Message::Ping(bytes::Bytes::new())).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> LiveRequestEvent {
        LiveRequestEvent {
            event_type: "request".into(),
            request_id: "req-1".into(),
            model: "test-model".into(),
            pool_id: "test-pool".into(),
            status_code: 200,
            latency_ms: 12,
            tokens: Some(3),
            ts: 1,
        }
    }

    #[tokio::test]
    async fn live_sender_can_be_initialized_inside_tokio_runtime() {
        // Regression test for the old implementation that called
        // `tokio::sync::Mutex::blocking_lock()` inside an async handler.
        // That panicked with "Cannot block the current thread from within
        // a runtime"; the OnceLock+broadcast::Sender approach must not.
        let mut receiver = get_live_tx().subscribe();
        let event = sample_event();

        broadcast_live_event(&event);
        let received = receiver.recv().await;
        assert_eq!(received.ok().as_deref(), Some(event.to_json().as_str()));
    }

    #[test]
    fn live_event_serializes_to_json() {
        let json: serde_json::Value =
            serde_json::from_str(&sample_event().to_json()).unwrap_or_default();
        assert_eq!(json["event_type"], "request");
        assert_eq!(json["status_code"], 200);
        assert_eq!(json["model"], "test-model");
        assert_eq!(json["pool_id"], "test-pool");
    }
}

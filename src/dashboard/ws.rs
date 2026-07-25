//! WebSocket handler for real-time dashboard updates.
//!
//! `GET /admin/ws` — upgrades to WebSocket, pushes key health snapshots every 5 seconds.

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<crate::server::AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, app_state: crate::server::AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(d))) => {
                        let _ = socket.send(Message::Pong(d)).await;
                    }
                    Some(Ok(Message::Text(_))) => {}
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            _ = interval.tick() => {
                let payload = build_ws_payload(&app_state).await;
                if let Ok(json) = serde_json::to_string(&payload)
                    && socket.send(Message::Text(json.into())).await.is_err()
                {
                    break;
                }
            }
        }
    }
}

async fn build_ws_payload(state: &crate::server::AppState) -> serde_json::Value {
    let snaps = state.router.current().pool_snapshot();
    let mut pools = Vec::new();

    for snap in snaps {
        let mut keys = Vec::new();
        for ks in snap.keys {
            let healthy = ks.healthy;
            keys.push(serde_json::json!({
                "key_hash": ks.key_hash,
                "weight": ks.weight,
                "healthy": healthy,
            }));
        }
        pools.push(serde_json::json!({
            "pool_id": snap.pool_id,
            "keys": keys,
        }));
    }

    use std::sync::atomic::Ordering;

    serde_json::json!({
        "type": "key_health",
        "ts": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        "pools": pools,
        "stats": {
            "requests_total": state.metrics.total_requests.load(Ordering::Relaxed),
            "failed": state.metrics.failed_requests.load(Ordering::Relaxed),
            "active": state.metrics.active_connections.load(Ordering::Relaxed),
        }
    })
}

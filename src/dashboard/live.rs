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

pub fn live_page_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>llm_proxy Live</title>
<style>
:root{--bg:#0d1117;--card:#161b22;--border:#30363d;--text:#c9d1d9;--green:#3fb950;--red:#f85149;--yellow:#d2991d;--blue:#58a6ff;--purple:#bc8cff}
*{margin:0;padding:0;box-sizing:border-box}
body{background:var(--bg);color:var(--text);font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',monospace;min-height:100vh}
header{background:var(--card);border-bottom:1px solid var(--border);padding:12px 20px;display:flex;align-items:center;justify-content:space-between}
header h1{font-size:18px;color:var(--blue)}
.stats{display:flex;gap:20px;font-size:12px}
.stat{display:flex;align-items:center;gap:6px}
.stat .dot{width:8px;height:8px;border-radius:50%}
.dot.green{background:var(--green);box-shadow:0 0 6px var(--green)}
.dot.red{background:var(--red);box-shadow:0 0 6px var(--red)}
main{padding:16px;max-width:1000px;margin:0 auto}
.event{background:var(--card);border:1px solid var(--border);border-radius:8px;padding:12px 16px;margin-bottom:8px;animation:slideIn .3s ease;display:flex;align-items:center;gap:16px}
@keyframes slideIn{from{opacity:0;transform:translateY(-10px)}to{opacity:1;transform:translateY(0)}}
.event .status{width:32px;height:32px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-weight:bold;font-size:14px}
.status.ok{background:rgba(63,185,80,.15);color:var(--green);border:2px solid var(--green)}
.status.err{background:rgba(248,81,73,.15);color:var(--red);border:2px solid var(--red)}
.event .info{flex:1}
.event .info .model{font-weight:600;font-size:14px}
.event .info .meta{font-size:12px;color:#8b949e;margin-top:2px}
.event .latency{font-size:13px;font-weight:600;text-align:right}
.latency.fast{color:var(--green)}
.latency.medium{color:var(--yellow)}
.latency.slow{color:var(--red)}
.event .tokens{font-size:12px;color:#8b949e;text-align:right}
.empty{text-align:center;padding:60px 20px;color:#8b949e;font-size:14px}
.summary{background:var(--card);border:1px solid var(--border);border-radius:8px;padding:16px;margin-bottom:16px;display:flex;gap:24px;font-size:13px}
.summary .item .val{font-weight:600;color:var(--blue)}
.summary .item .lbl{color:#8b949e;font-size:11px}
</style>
</head>
<body>
<header>
<h1>llm_proxy Live</h1>
<div class="stats">
<div class="stat"><span class="dot green" id="ws-dot"></span><span id="conn-status">Connecting...</span></div>
<div class="stat">QPS: <span id="qps">0</span></div>
</div>
</header>
<main>
<div class="summary" id="summary">
<div class="item"><div class="val" id="total-reqs">0</div><div class="lbl">Total</div></div>
<div class="item"><div class="val" id="total-ok">0</div><div class="lbl">200</div></div>
<div class="item"><div class="val" id="total-err">0</div><div class="lbl">Errors</div></div>
<div class="item"><div class="val" id="p50">-ms</div><div class="lbl">P50</div></div>
<div class="item"><div class="val" id="p99">-ms</div><div class="lbl">P99</div></div>
</div>
<div id="events"></div>
<div class="empty" id="empty">Waiting for requests...</div>
</main>
<script>
const ws = new WebSocket(`ws://${location.host}/admin/live`);
const dot = document.getElementById('ws-dot');
const status = document.getElementById('conn-status');
const events = document.getElementById('events');
const empty = document.getElementById('empty');
let latencies = [];
let totalReqs = 0, totalOk = 0, totalErr = 0, qpsCount = 0;
const secondLatencies = [];

ws.onopen = () => {
    dot.className = 'dot green';
    status.textContent = 'Connected';
};
ws.onclose = () => {
    dot.className = 'dot red';
    status.textContent = 'Disconnected';
};
ws.onmessage = (evt) => {
    try {
        const e = JSON.parse(evt.data);
        if (e.event_type === 'lag') return;
        totalReqs++;
        qpsCount++;
        if (e.status_code >= 200 && e.status_code < 300) totalOk++;
        else totalErr++;
        if (e.latency_ms > 0) {
            latencies.push(e.latency_ms);
            secondLatencies.push(e.latency_ms);
            if (latencies.length > 1000) latencies.shift();
        }
        updateStats();
        renderEvent(e);
    } catch(_) {}
};

function updateStats() {
    document.getElementById('total-reqs').textContent = totalReqs;
    document.getElementById('total-ok').textContent = totalOk;
    document.getElementById('total-err').textContent = totalErr;
    if (latencies.length > 0) {
        const sorted = [...latencies].sort((a,b) => a-b);
        const p50 = sorted[Math.floor(sorted.length * 0.5)];
        const p99 = sorted[Math.floor(sorted.length * 0.99)];
        document.getElementById('p50').textContent = `${p50}ms`;
        document.getElementById('p99').textContent = `${p99}ms`;
    }
}

function renderEvent(e) {
    empty.style.display = 'none';
    const cls = e.status_code >= 200 && e.status_code < 300 ? 'ok' : 'err';
    let latCls = 'fast';
    if (e.latency_ms > 2000) latCls = 'slow';
    else if (e.latency_ms > 500) latCls = 'medium';
    const div = document.createElement('div');
    div.className = 'event';
    div.innerHTML = `<div class="status ${cls}">${e.status_code}</div>
        <div class="info">
            <div class="model">${e.model} <span style="color:#8b949e;font-weight:normal">@ ${e.pool_id}</span></div>
            <div class="meta">${e.request_id.substring(0,8)}</div>
        </div>
        <div>
            <div class="latency ${latCls}">${e.latency_ms}ms</div>
            ${e.tokens ? `<div class="tokens">${e.tokens} tokens</div>` : ''}
        </div>`;
    if (events.firstChild) {
        events.insertBefore(div, events.firstChild);
    } else {
        events.appendChild(div);
    }
    if (events.children.length > 100) events.removeChild(events.lastChild);
}

setInterval(() => {
    document.getElementById('qps').textContent = qpsCount;
    qpsCount = 0;
}, 1000);
</script>
</body>
</html>"#
    .to_string()
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

    #[test]
    fn live_page_html_renders() {
        let html = live_page_html();
        assert!(html.contains("<title>llm_proxy Live</title>"));
        assert!(html.contains("/admin/live"));
    }
}

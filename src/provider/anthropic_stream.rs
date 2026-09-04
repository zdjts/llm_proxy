//! Anthropic SSE → OpenAI-compatible SSE relay.
//!
//! Transforms Anthropic Messages streaming API SSE events into OpenAI-compatible
//! `chat.completion.chunk` SSE chunks on-the-fly.

use bytes::Bytes;
use futures::Stream;
use futures::stream::BoxStream;
use serde_json::Value;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use crate::error::AppError;

struct RelayState {
    message_id: String,
    model: String,
    created: u64,
    content_idx: usize,
    streamed_tokens: u32,
    finish_reason: Option<String>,
    buffer: Vec<u8>,
    pending: Vec<Bytes>,
    done: bool,
}

impl RelayState {
    fn new(model: &str) -> Self {
        Self {
            message_id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            model: model.to_owned(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            content_idx: 0,
            streamed_tokens: 0,
            finish_reason: None,
            buffer: Vec::new(),
            pending: Vec::new(),
            done: false,
        }
    }

    fn feed_bytes(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let line = self.buffer[..pos].to_vec();
            self.buffer.drain(..=pos);
            let line_trimmed = std::str::from_utf8(&line).unwrap_or("").trim().to_owned();
            if line_trimmed.is_empty() {
                continue;
            }
            self.process_line(&line_trimmed);
        }
    }

    fn process_line(&mut self, line: &str) {
        let data = match line.strip_prefix("data: ") {
            Some(d) => d,
            _ => return,
        };
        if data.is_empty() {
            return;
        }

        let value: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return,
        };

        let ev_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match ev_type {
            "message_start" => {
                if let Some(msg) = value.get("message") {
                    if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                        self.message_id = id.to_owned();
                    }
                    if let Some(model) = msg.get("model").and_then(|v| v.as_str()) {
                        self.model = model.to_owned();
                    }
                }
                if let Some(chunk) = self.emit_chunk("assistant", None, None) {
                    self.pending.push(chunk);
                }
            }
            "content_block_delta" => {
                let delta = value.get("delta");
                let delta_type = delta
                    .and_then(|d| d.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                let text = if delta_type == "thinking_delta" {
                    delta
                        .and_then(|d| d.get("thinking"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                } else {
                    delta
                        .and_then(|d| d.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                };
                self.content_idx =
                    value.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                if let Some(chunk) =
                    self.emit_chunk(text, None, (delta_type == "thinking_delta").then_some(true))
                {
                    self.pending.push(chunk);
                }
            }
            "message_delta" => {
                self.finish_reason = value
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|s| s.as_str())
                    .map(|s| match s {
                        "end_turn" => "stop",
                        "max_tokens" => "length",
                        "stop_sequence" => "stop",
                        "tool_use" => "tool_calls",
                        _ => "stop",
                    })
                    .map(String::from);

                if let Some(usage) = value.get("usage")
                    && let Some(ot) = usage.get("output_tokens").and_then(|v| v.as_u64())
                {
                    self.streamed_tokens = ot as u32;
                }

                if let Some(chunk) = self.emit_chunk("", self.finish_reason.as_deref(), None) {
                    self.pending.push(chunk);
                }
            }
            "message_stop" => {
                let payload = serde_json::json!({
                    "id": self.message_id,
                    "object": "chat.completion.chunk",
                    "created": self.created,
                    "model": self.model,
                    "choices": [{
                        "index": self.content_idx,
                        "delta": {},
                        "finish_reason": self.finish_reason,
                    }],
                    "usage": {
                        "completion_tokens": self.streamed_tokens,
                        "prompt_tokens": 0,
                        "total_tokens": self.streamed_tokens,
                    }
                });
                if let Ok(s) = serde_json::to_string(&payload) {
                    self.pending.push(Bytes::from(format!("data: {s}\n\n")));
                }
                self.done = true;
            }
            "ping" | "content_block_start" | "content_block_stop" => {}
            _ => {}
        }
    }

    fn emit_chunk(
        &self,
        text: &str,
        finish_reason: Option<&str>,
        reasoning: Option<bool>,
    ) -> Option<Bytes> {
        let delta = if text == "assistant" {
            serde_json::json!({"role": "assistant"})
        } else if reasoning == Some(true) {
            serde_json::json!({"reasoning_content": text})
        } else if text.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::json!({"content": text})
        };

        let payload = serde_json::json!({
            "id": self.message_id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{
                "index": self.content_idx,
                "delta": delta,
                "finish_reason": finish_reason,
            }],
        });

        if let Ok(s) = serde_json::to_string(&payload) {
            Some(Bytes::from(format!("data: {s}\n\n")))
        } else {
            None
        }
    }

    fn next_chunk(&mut self) -> Option<Bytes> {
        if self.pending.is_empty() {
            None
        } else {
            Some(self.pending.remove(0))
        }
    }
}

/// A stream that wraps an upstream Anthropic SSE byte stream and transforms it
/// into OpenAI-compatible SSE chunks.
pub struct AnthropicStreamRelay {
    upstream: BoxStream<'static, Result<Bytes, AppError>>,
    state: Arc<Mutex<RelayState>>,
    upstream_done: bool,
}

impl AnthropicStreamRelay {
    pub fn new(upstream: BoxStream<'static, Result<Bytes, AppError>>, model: &str) -> Self {
        Self {
            upstream,
            state: Arc::new(Mutex::new(RelayState::new(model))),
            upstream_done: false,
        }
    }
}

impl Stream for AnthropicStreamRelay {
    type Item = Result<Bytes, AppError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let state_arc = Arc::clone(&self.state);
        let mut guard = match state_arc.lock() {
            Ok(g) => g,
            Err(_) => return Poll::Ready(Some(Err(AppError::Internal("lock poisoned".into())))),
        };

        if let Some(chunk) = guard.next_chunk() {
            return Poll::Ready(Some(Ok(chunk)));
        }

        if guard.done {
            return Poll::Ready(None);
        }

        drop(guard);

        if self.upstream_done {
            return Poll::Ready(None);
        }

        match self.upstream.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                let state_arc2 = Arc::clone(&self.state);
                let mut guard = match state_arc2.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        return Poll::Ready(Some(Err(AppError::Internal("lock poisoned".into()))));
                    }
                };
                guard.feed_bytes(&bytes);
                if let Some(chunk) = guard.next_chunk() {
                    Poll::Ready(Some(Ok(chunk)))
                } else {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
            Poll::Ready(Some(Err(e))) => {
                self.upstream_done = true;
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                self.upstream_done = true;
                let state_arc2 = Arc::clone(&self.state);
                let mut guard = match state_arc2.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        return Poll::Ready(Some(Err(AppError::Internal("lock poisoned".into()))));
                    }
                };
                if let Some(chunk) = guard.next_chunk() {
                    Poll::Ready(Some(Ok(chunk)))
                } else if !guard.done {
                    guard.pending.push(Bytes::from("data: [DONE]\n\n"));
                    guard.done = true;
                    Poll::Ready(Some(Ok(Bytes::from("data: [DONE]\n\n"))))
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Convenience function to create a relayed stream.
pub fn relay_anthropic_stream(
    upstream: BoxStream<'static, Result<Bytes, AppError>>,
    model: &str,
) -> BoxStream<'static, Result<Bytes, AppError>> {
    Box::pin(AnthropicStreamRelay::new(upstream, model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[test]
    fn relay_state_parses_message_start() {
        let mut state = RelayState::new("claude");
        state.process_line(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_001\",\"model\":\"claude\"}}",
        );
        let chunk = state.next_chunk();
        assert!(chunk.is_some());
        let binding = chunk.unwrap();
        let text = String::from_utf8_lossy(&binding);
        assert!(text.contains("\"role\":\"assistant\""));
    }

    #[test]
    fn relay_state_parses_content_delta() {
        let mut state = RelayState::new("claude");
        state.process_line(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_001\",\"model\":\"claude\"}}",
        );
        let _ = state.next_chunk();
        state.process_line(
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}",
        );
        let chunk = state.next_chunk();
        assert!(chunk.is_some());
        let binding = chunk.unwrap();
        let text = String::from_utf8_lossy(&binding);
        assert!(text.contains("Hello"));
    }

    #[test]
    fn relay_state_parses_message_delta_stop() {
        let mut state = RelayState::new("claude");
        state.process_line(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_001\",\"model\":\"claude\"}}",
        );
        let _ = state.next_chunk();
        state.process_line(
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":42}}",
        );
        let chunk = state.next_chunk();
        assert!(chunk.is_some());
        let binding = chunk.unwrap();
        let text = String::from_utf8_lossy(&binding);
        assert!(text.contains("\"finish_reason\":\"stop\""));
    }

    #[test]
    fn relay_state_skips_ping() {
        let mut state = RelayState::new("claude");
        state.process_line("data: {\"type\":\"ping\"}");
        assert!(state.next_chunk().is_none());
    }

    #[test]
    fn relay_state_parses_message_stop() {
        let mut state = RelayState::new("claude");
        state.process_line(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_001\",\"model\":\"claude\"}}",
        );
        let _ = state.next_chunk();
        state.process_line(
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":42}}",
        );
        let _ = state.next_chunk();
        state.process_line("data: {\"type\":\"message_stop\"}");
        let chunk = state.next_chunk();
        assert!(chunk.is_some());
        let binding = chunk.unwrap();
        let text = String::from_utf8_lossy(&binding);
        assert!(text.contains("\"usage\""));
    }

    #[tokio::test]
    async fn stream_relay_transforms_simple_flow() {
        let input = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_001","model":"claude"}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}

event: message_stop
data: {"type":"message_stop"}
"#;

        let upstream: BoxStream<'static, Result<Bytes, AppError>> =
            Box::pin(stream::once(futures::future::ready(Ok(Bytes::from(input)))));
        let mut relay = AnthropicStreamRelay::new(upstream, "claude");

        use futures::StreamExt;
        let mut chunks = Vec::new();
        while let Some(Ok(chunk)) = relay.next().await {
            chunks.push(chunk);
        }

        assert!(!chunks.is_empty());
        let combined = chunks
            .iter()
            .map(|c| String::from_utf8_lossy(c))
            .collect::<Vec<_>>()
            .join("");
        assert!(combined.contains("Hello"));
        assert!(combined.contains("world"));
        assert!(combined.contains("\"finish_reason\":\"stop\""));
    }
}

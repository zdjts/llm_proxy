//! Gemini SSE → OpenAI-compatible SSE relay.
//!
//! The Gemini `streamGenerateContent?alt=sse` returns a stream of JSON objects
//! (one per line), each of the form:
//! ```json
//! {"candidates":[{"content":{"role":"model","parts":[{"text":"Hello"}]},"finishReason":null}],
//!  "usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":0,"totalTokenCount":10}}
//! ```

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
    finish_reason: Option<String>,
    buffer: Vec<u8>,
    pending: Vec<Bytes>,
    streamed_tokens: u32,
    prompt_tokens: u32,
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
            finish_reason: None,
            buffer: Vec::new(),
            pending: Vec::new(),
            streamed_tokens: 0,
            prompt_tokens: 0,
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
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return,
        };

        if let Some(usage) = value.get("usageMetadata") {
            if let Some(pt) = usage.get("promptTokenCount").and_then(|v| v.as_u64()) {
                self.prompt_tokens = pt as u32;
            }
            if let Some(ct) = usage.get("candidatesTokenCount").and_then(|v| v.as_u64()) {
                self.streamed_tokens = ct as u32;
            }
        }

        if let Some(candidates) = value.get("candidates").and_then(|v| v.as_array()) {
            for candidate in candidates {
                let content = match candidate.get("content") {
                    Some(c) => c,
                    None => continue,
                };
                let parts = match content.get("parts").and_then(|v| v.as_array()) {
                    Some(p) => p,
                    None => continue,
                };

                let mut reasoning = false;
                let mut text = String::new();
                for part in parts {
                    if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                        text.push_str(t);
                    }
                    reasoning |= part
                        .get("thought")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                }

                let finish_reason = candidate
                    .get("finishReason")
                    .and_then(|v| v.as_str())
                    .filter(|s| *s != "null")
                    .map(|s| match s {
                        "STOP" => "stop",
                        "MAX_TOKENS" => "length",
                        "SAFETY" | "RECITATION" => "content_filter",
                        _ => "stop",
                    })
                    .map(String::from);

                if finish_reason.is_some() && finish_reason != self.finish_reason {
                    self.finish_reason = finish_reason;
                }

                let is_first = self.pending.is_empty();

                if is_first && let Some(chunk) = self.emit_chunk("assistant", None, false) {
                    self.pending.push(chunk);
                }

                if !text.is_empty() {
                    if let Some(chunk) =
                        self.emit_chunk(&text, self.finish_reason.as_deref(), reasoning)
                    {
                        self.pending.push(chunk);
                    }
                } else if self.finish_reason.is_some()
                    && let Some(chunk) = self.emit_chunk("", self.finish_reason.as_deref(), false)
                {
                    self.pending.push(chunk);
                }

                if self.finish_reason.is_some() {
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
                            "prompt_tokens": self.prompt_tokens,
                            "total_tokens": self.prompt_tokens + self.streamed_tokens,
                        }
                    });
                    if let Ok(s) = serde_json::to_string(&payload) {
                        self.pending.push(Bytes::from(format!("data: {s}\n\n")));
                    }
                    self.done = true;
                }
            }
        }
    }

    fn emit_chunk(
        &self,
        text: &str,
        finish_reason: Option<&str>,
        reasoning: bool,
    ) -> Option<Bytes> {
        let delta = if text == "assistant" {
            serde_json::json!({"role": "assistant"})
        } else if reasoning {
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

pub struct GeminiStreamRelay {
    upstream: BoxStream<'static, Result<Bytes, AppError>>,
    state: Arc<Mutex<RelayState>>,
    upstream_done: bool,
}

impl GeminiStreamRelay {
    pub fn new(upstream: BoxStream<'static, Result<Bytes, AppError>>, model: &str) -> Self {
        Self {
            upstream,
            state: Arc::new(Mutex::new(RelayState::new(model))),
            upstream_done: false,
        }
    }
}

impl Stream for GeminiStreamRelay {
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

pub fn relay_gemini_stream(
    upstream: BoxStream<'static, Result<Bytes, AppError>>,
    model: &str,
) -> BoxStream<'static, Result<Bytes, AppError>> {
    Box::pin(GeminiStreamRelay::new(upstream, model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[test]
    fn relay_state_parses_text_chunk() {
        let mut state = RelayState::new("gemini-pro");
        state.process_line(
            "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hello\"}]},\"finishReason\":null}]}",
        );
        let first = state.next_chunk();
        assert!(first.is_some());
        assert!(String::from_utf8_lossy(&first.unwrap()).contains("\"role\":\"assistant\""));

        let second = state.next_chunk();
        assert!(second.is_some());
        assert!(String::from_utf8_lossy(&second.unwrap()).contains("Hello"));
    }

    #[test]
    fn relay_state_parses_finish_reason() {
        let mut state = RelayState::new("gemini-pro");
        state.process_line(
            "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hi\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":1,\"totalTokenCount\":6}}"
        );
        let _ = state.next_chunk(); // role
        let _ = state.next_chunk(); // content
        let final_chunk = state.next_chunk();
        assert!(final_chunk.is_some());
        let binding = final_chunk.unwrap();
        let text = String::from_utf8_lossy(&binding);
        assert!(text.contains("\"finish_reason\":\"stop\""));
        assert!(text.contains("\"total_tokens\":6"));
    }

    #[tokio::test]
    async fn stream_relay_transforms_gemini_sse() {
        let input = "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hello\"}]},\"finishReason\":null}]}\n{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\" world\"}]},\"finishReason\":null}]}\n{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"!\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":3,\"totalTokenCount\":8}}\n";

        let upstream: Pin<Box<dyn Stream<Item = Result<Bytes, AppError>> + Send>> =
            Box::pin(stream::once(futures::future::ready(Ok(Bytes::from(input)))));
        let mut relay = GeminiStreamRelay::new(upstream, "gemini-pro");

        use futures::StreamExt;
        let mut chunks = Vec::new();
        while let Some(Ok(chunk)) = relay.next().await {
            chunks.push(chunk);
        }

        assert!(chunks.len() >= 3);
        let combined = chunks
            .iter()
            .map(|c| String::from_utf8_lossy(c))
            .collect::<Vec<_>>()
            .join("");
        assert!(combined.contains("Hello"));
        assert!(combined.contains("world"));
        assert!(combined.contains("!"));
        assert!(combined.contains("\"finish_reason\":\"stop\""));
    }
}

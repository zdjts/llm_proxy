//! SSE stream normaliser — the streaming counterpart to
//! [`crate::response_normalize`].  Lives outside the `provider/` tree
//! and is provider-agnostic.
//!
//! For each complete SSE message we:
//!
//! 1. Parse `data:` JSON as a `serde_json::Value` so unknown fields
//!    survive.
//! 2. Fold `reasoning` / `reasoning_text` / string `thinking` into
//!    `delta.reasoning_content`.
//! 3. If `delta.content` contains a built-in think wrapper, split it
//!    with [`ThinkTagAccumulator`].  Existing `reasoning_content` is
//!    kept — never dropped when emitting text.
//! 4. Pass comments, `[DONE]`, and unparseable payloads through.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use futures::Stream;
use serde_json::{Map, Value};

use crate::error::AppError;
use crate::response_normalize::{ThinkEvent, ThinkTagAccumulator, builtin_think_pairs};

pub struct NormalizingStream<S> {
    inner: S,
    acc: ThinkTagAccumulator,
    pending: BytesMut,
    seen_done: bool,
    inner_eof: bool,
    completed: bool,
}

impl<S> NormalizingStream<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            acc: ThinkTagAccumulator::new(builtin_think_pairs()),
            pending: BytesMut::new(),
            seen_done: false,
            inner_eof: false,
            completed: false,
        }
    }
}

impl<S> Stream for NormalizingStream<S>
where
    S: Stream<Item = Result<Bytes, AppError>> + Unpin,
{
    type Item = Result<Bytes, AppError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if self.completed {
                return Poll::Ready(None);
            }
            if let Some(out) = self.process_pending() {
                return Poll::Ready(Some(Ok(out)));
            }
            if self.inner_eof {
                self.completed = true;
                return Poll::Ready(self.flush_eof().map(Ok));
            }

            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    self.pending.extend_from_slice(&bytes);
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                Poll::Ready(None) => {
                    self.inner_eof = true;
                    if !self.pending.is_empty() && !self.pending.ends_with(b"\n") {
                        self.pending.extend_from_slice(b"\n");
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<S> NormalizingStream<S> {
    /// Split on each complete line (`\n`), not on SSE blank-line frames.
    /// Many OpenAI-compatible proxies flush `data: {…}\n` without the extra
    /// empty line; waiting for `\n\n` would hold the entire completion and
    /// look like a dropped socket to the client.
    fn process_pending(&mut self) -> Option<Bytes> {
        loop {
            let end = self.pending.iter().position(|&b| b == b'\n')? + 1;
            let raw = self.pending.split_to(end);
            let line = raw.freeze();
            if let Some(transformed) = self.transform_line(&line) {
                return Some(transformed);
            }
        }
    }

    fn transform_line(&mut self, line: &Bytes) -> Option<Bytes> {
        let text = match std::str::from_utf8(line) {
            Ok(s) => s,
            Err(_) => return Some(line.clone()),
        };
        let line_no_nl = text.strip_suffix('\n').unwrap_or(text);
        let line_no_cr = line_no_nl.strip_suffix('\r').unwrap_or(line_no_nl);
        if line_no_cr.is_empty() {
            return None;
        }
        if let Some(payload) = line_no_cr.strip_prefix("data:") {
            let payload = payload.trim_start();
            if payload == "[DONE]" {
                self.seen_done = true;
                return Some(Bytes::from_static(b"data: [DONE]\n\n"));
            }
            match serde_json::from_str::<Value>(payload) {
                Ok(mut chunk) if chunk.is_object() => {
                    if rewrite_chunk_value(&mut self.acc, &mut chunk)
                        && let Ok(s) = serde_json::to_string(&chunk)
                    {
                        let mut out = Vec::with_capacity(s.len() + 8);
                        out.extend_from_slice(b"data: ");
                        out.extend_from_slice(s.as_bytes());
                        out.extend_from_slice(b"\n\n");
                        return Some(Bytes::from(out));
                    }
                    None
                }
                _ => Some(sse_line(line_no_cr)),
            }
        } else {
            Some(sse_line(line_no_cr))
        }
    }

    fn flush_eof(&mut self) -> Option<Bytes> {
        let as_reasoning = self.acc.in_reasoning() || self.acc.tag_count() > 0;
        let flushed_acc = self.acc.flush();
        let seen_done = self.seen_done;
        self.seen_done = true;
        assemble_drained(&flushed_acc, as_reasoning, seen_done)
    }
}

fn sse_line(line: &str) -> Bytes {
    let mut out = Vec::with_capacity(line.len() + 2);
    out.extend_from_slice(line.as_bytes());
    out.extend_from_slice(b"\n\n");
    Bytes::from(out)
}

const REASONING_ALIASES: [&str; 3] = ["reasoning_content", "reasoning", "reasoning_text"];

fn fold_reasoning_aliases(delta: &mut Map<String, Value>) {
    let mut parts = Vec::new();
    for key in REASONING_ALIASES {
        if let Some(Value::String(s)) = delta.remove(key)
            && !s.is_empty()
        {
            parts.push(s);
        }
    }
    if let Some(Value::String(s)) = delta.get("thinking")
        && !s.is_empty()
    {
        parts.push(s.clone());
        delta.remove("thinking");
    }
    if !parts.is_empty() {
        delta.insert(
            "reasoning_content".to_string(),
            Value::String(parts.concat()),
        );
    }
}

fn append_reasoning(delta: &mut Map<String, Value>, extra: String) {
    if extra.is_empty() {
        return;
    }
    match delta.get_mut("reasoning_content") {
        Some(Value::String(existing)) => existing.push_str(&extra),
        _ => {
            delta.insert("reasoning_content".to_string(), Value::String(extra));
        }
    }
}

fn delta_has_payload(delta: &Map<String, Value>) -> bool {
    delta.iter().any(|(_, v)| match v {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => true,
    })
}

/// Returns whether the chunk still has anything worth forwarding.
fn rewrite_chunk_value(acc: &mut ThinkTagAccumulator, chunk: &mut Value) -> bool {
    let mut emit = chunk.get("usage").is_some();
    let Some(choices) = chunk.get_mut("choices").and_then(|c| c.as_array_mut()) else {
        return true;
    };
    if choices.is_empty() {
        return true;
    }
    for choice in choices.iter_mut() {
        if choice
            .get("finish_reason")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty())
        {
            emit = true;
        }
        let Some(delta) = choice.get_mut("delta").and_then(|d| d.as_object_mut()) else {
            emit = true;
            continue;
        };
        fold_reasoning_aliases(delta);

        let content = match delta.get("content") {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        if let Some(content) = content {
            let events = acc.feed(&content);
            let mut text = String::new();
            let mut reasoning = String::new();
            for ev in events {
                match ev {
                    ThinkEvent::Text(s) => text.push_str(&s),
                    ThinkEvent::Reasoning(s) => reasoning.push_str(&s),
                    ThinkEvent::Pending => {}
                }
            }
            if text.is_empty() {
                delta.remove("content");
            } else {
                delta.insert("content".to_string(), Value::String(text));
            }
            append_reasoning(delta, reasoning);
        }

        if delta_has_payload(delta) {
            emit = true;
        }
    }
    emit
}

fn assemble_drained(flushed_acc: &str, as_reasoning: bool, seen_done: bool) -> Option<Bytes> {
    let mut out: Vec<u8> = Vec::new();
    if !flushed_acc.is_empty() {
        let field = if as_reasoning {
            "reasoning_content"
        } else {
            "content"
        };
        let payload = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": { field: flushed_acc },
            }],
        });
        if let Ok(s) = serde_json::to_string(&payload) {
            out.extend_from_slice(b"data: ");
            out.extend_from_slice(s.as_bytes());
            out.extend_from_slice(b"\n\n");
        }
    }
    if !seen_done {
        out.extend_from_slice(b"data: [DONE]\n\n");
    }
    if out.is_empty() {
        None
    } else {
        Some(Bytes::from(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use futures::stream;

    fn sse_chunk(id: &str, content: &str) -> String {
        format!(
            "data: {{\"id\":\"{id}\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{content}\"}}}}]}}\n\n"
        )
    }

    fn chunk_payload(payload: &str) -> String {
        format!("data: {payload}\n\n")
    }

    async fn collect_joined<S>(s: S) -> String
    where
        S: Stream<Item = Result<Bytes, AppError>> + Unpin,
    {
        let mut collected = Vec::new();
        let mut stream = s;
        while let Some(b) = stream.next().await {
            collected.push(b.unwrap());
        }
        String::from_utf8(collected.concat()).unwrap()
    }

    fn accumulate_fields(joined: &str) -> (String, String) {
        let mut reasoning = String::new();
        let mut text = String::new();
        for line in joined.lines() {
            if let Some(rest) = line.strip_prefix("data: ") {
                if rest == "[DONE]" {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(rest) {
                    if let Some(s) = v
                        .pointer("/choices/0/delta/reasoning_content")
                        .and_then(|x| x.as_str())
                    {
                        reasoning.push_str(s);
                    }
                    if let Some(s) = v
                        .pointer("/choices/0/delta/content")
                        .and_then(|x| x.as_str())
                    {
                        text.push_str(s);
                    }
                }
            }
        }
        (reasoning, text)
    }

    #[tokio::test]
    async fn passthrough_when_no_chunks_match_a_tag() {
        let upstream = stream::iter(vec![Ok(Bytes::from(sse_chunk("1", "hello world")))]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        assert!(
            joined.contains(r#""content":"hello world""#),
            "got: {joined}"
        );
    }

    #[tokio::test]
    async fn single_chunk_with_single_block_splits_reasoning_and_text() {
        let upstream = stream::iter(vec![Ok(Bytes::from(sse_chunk(
            "1",
            "<think>hidden</think>visible",
        )))]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        let (reasoning, text) = accumulate_fields(&joined);
        assert_eq!(reasoning, "hidden");
        assert_eq!(text, "visible");
        assert!(
            !joined.contains("<think>") && !joined.contains("</think>"),
            "think tags should be stripped from output: {joined}"
        );
    }

    #[tokio::test]
    async fn tag_split_across_chunks_emits_correct_sequence() {
        let upstream = stream::iter(vec![
            Ok(Bytes::from(sse_chunk("1", "<th"))),
            Ok(Bytes::from(sse_chunk("2", "ink>hidden</think>visible"))),
        ]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        let (reasoning, text) = accumulate_fields(&joined);
        assert_eq!(reasoning, "hidden");
        assert_eq!(text, "visible");
        assert!(!joined.contains("<think>"));
    }

    #[tokio::test]
    async fn multiple_blocks_in_one_chunk_join_reasoning_and_text() {
        let upstream = stream::iter(vec![Ok(Bytes::from(sse_chunk(
            "1",
            "<think>A</think>mid<think>B</think>tail",
        )))]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        let (reasoning, text) = accumulate_fields(&joined);
        assert_eq!(reasoning, "AB");
        assert_eq!(text, "midtail");
        assert!(
            !joined.contains("<think>") && !joined.contains("</think>"),
            "tags should be stripped: {joined}"
        );
    }

    #[tokio::test]
    async fn usage_chunk_passes_through() {
        let payload = chunk_payload(
            r#"{"id":"u","object":"chat.completion.chunk","created":1,"model":"m","choices":[],"usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}}"#,
        );
        let upstream = stream::iter(vec![Ok(Bytes::from(payload))]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        assert!(joined.contains(r#""prompt_tokens":3"#));
        assert!(joined.contains(r#""completion_tokens":5"#));
        assert!(joined.contains(r#""total_tokens":8"#));
    }

    #[tokio::test]
    async fn done_marker_is_passed_through() {
        let upstream = stream::iter(vec![
            Ok(Bytes::from(sse_chunk("1", "no tags here"))),
            Ok(Bytes::from("data: [DONE]\n\n".to_string())),
        ]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        assert!(joined.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn comment_lines_pass_through() {
        let upstream = stream::iter(vec![Ok(Bytes::from(": keep-alive\n\n".to_string()))]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        assert!(joined.contains(": keep-alive"));
    }

    #[tokio::test]
    async fn unparseable_data_line_passes_through() {
        let upstream = stream::iter(vec![Ok(Bytes::from(
            "data: not-json-content\n\n".to_string(),
        ))]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        assert!(joined.contains("data: not-json-content"));
    }

    #[tokio::test]
    async fn unclosed_tag_at_end_emits_pending_then_flushes() {
        let upstream = stream::iter(vec![Ok(Bytes::from(sse_chunk(
            "1",
            "<think>half-formed reasoning body",
        )))]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        assert!(
            joined.contains("reasoning_content"),
            "expected reasoning_content on flush: {joined}"
        );
        assert!(
            joined.contains("half-formed reasoning body"),
            "expected held bytes in reasoning_content: {joined}"
        );
    }

    #[tokio::test]
    async fn keeps_existing_reasoning_when_emitting_text() {
        let payload = chunk_payload(
            r#"{"id":"1","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"content":"visible","reasoning_content":"hidden"}}]}"#,
        );
        let upstream = stream::iter(vec![Ok(Bytes::from(payload))]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        let (reasoning, text) = accumulate_fields(&joined);
        assert_eq!(reasoning, "hidden");
        assert_eq!(text, "visible");
    }

    #[tokio::test]
    async fn folds_reasoning_alias_into_reasoning_content() {
        let payload = chunk_payload(
            r#"{"id":"1","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"reasoning":"plan"}}]}"#,
        );
        let upstream = stream::iter(vec![Ok(Bytes::from(payload))]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        let (reasoning, text) = accumulate_fields(&joined);
        assert_eq!(reasoning, "plan");
        assert_eq!(text, "");
        assert!(
            !joined.contains(r#""reasoning":"plan""#),
            "alias should be folded away: {joined}"
        );
    }

    #[tokio::test]
    async fn crlf_frames_are_recognised() {
        let payload = "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"<think>hid</think>vis\"}}]}\r\n\r\n";
        let upstream = stream::iter(vec![Ok(Bytes::from(payload))]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        let (reasoning, text) = accumulate_fields(&joined);
        assert_eq!(reasoning, "hid");
        assert_eq!(text, "vis");
    }

    #[tokio::test]
    async fn streaming_tag_spans_three_chunks() {
        let upstream = stream::iter(vec![
            Ok(Bytes::from(sse_chunk("1", "<thi"))),
            Ok(Bytes::from(sse_chunk("2", "n"))),
            Ok(Bytes::from(sse_chunk("3", "k>rest</think>answer"))),
        ]);
        let joined = collect_joined(NormalizingStream::new(upstream)).await;
        let (reasoning, text) = accumulate_fields(&joined);
        assert_eq!(reasoning, "rest");
        assert_eq!(text, "answer");
    }

    struct OneChunkThenHang(Option<Bytes>);

    impl Stream for OneChunkThenHang {
        type Item = Result<Bytes, AppError>;
        fn poll_next(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            match self.get_mut().0.take() {
                Some(b) => std::task::Poll::Ready(Some(Ok(b))),
                None => std::task::Poll::Pending,
            }
        }
    }

    #[tokio::test]
    async fn single_lf_emits_before_upstream_closes() {
        // Proxies often flush `data: {…}\n` without a blank line.  Holding
        // until `\n\n` or EOF makes pi's fetch look like a dropped socket.
        let line = "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n";
        let mut s = NormalizingStream::new(OneChunkThenHang(Some(Bytes::from(line))));
        let first = tokio::time::timeout(std::time::Duration::from_millis(400), s.next())
            .await
            .expect("must emit on a single-LF data line without waiting for EOF")
            .unwrap()
            .unwrap();
        let text = String::from_utf8(first.to_vec()).unwrap();
        assert!(text.contains("hello"), "{text}");
    }
}

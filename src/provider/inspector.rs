//! Side-channel SSE stream usage accumulator.
//!
//! [`StreamInspector`] parses `data:` lines from an upstream SSE event stream,
//! accumulating token usage and finish-reason without re-serializing or
//! blocking the main byte-forwarding path.

use std::time::Instant;

use crate::audit::{AuditFromProvider, CacheReport, ProviderCacheKind};
use crate::types::Usage;

/// Parsed summary collected from a streaming chat completion response.
pub struct StreamSummary {
    pub usage: Option<Usage>,
    pub finish_reason: Option<String>,
    pub upstream_model: Option<String>,
    pub raw_usage_json: Option<serde_json::Value>,
}

pub struct StreamInspector {
    accumulated_usage: Option<Usage>,
    finish_reason: Option<String>,
    upstream_model: Option<String>,
    raw_usage_json: Option<serde_json::Value>,
    first_chunk_at: Option<Instant>,
    start_at: Instant,
    reasoning_tokens: i64,
    audio_tokens: i64,
    cache_hit_tokens: Option<i64>,
    cache_source: ProviderCacheKind,
}

impl StreamInspector {
    pub fn new() -> Self {
        Self::new_started_at(Instant::now())
    }

    /// Inspector whose TTFT clock starts at `start_at` instead of now.
    ///
    /// The handler passes the moment the client request arrived so that TTFT
    /// covers the full client-perceived wait (connection + upstream headers +
    /// time to first token).  Measuring only from response-header receipt
    /// yields ~0 ms for upstreams that buffer the entire SSE body and flush
    /// it at the end of generation.
    pub fn new_started_at(start_at: Instant) -> Self {
        Self {
            accumulated_usage: None,
            finish_reason: None,
            upstream_model: None,
            raw_usage_json: None,
            first_chunk_at: None,
            start_at,
            reasoning_tokens: 0,
            audio_tokens: 0,
            cache_hit_tokens: None,
            cache_source: ProviderCacheKind::None,
        }
    }

    /// Time from construction to first ingested chunk in milliseconds.
    /// Returns `None` if no chunks have been ingested yet (or the stream
    /// was empty).  Only meaningful for streaming responses.
    ///
    /// With [`StreamInspector::new_started_at`] this measures from the
    /// supplied start instant (normally client-request arrival), i.e. the
    /// full client-perceived time-to-first-token.
    pub fn ttft_ms(&self) -> Option<i64> {
        self.first_chunk_at
            .map(|at| at.duration_since(self.start_at).as_millis() as i64)
    }

    /// Feed one raw SSE line (e.g. `data: {"choices":...}\n`).
    ///
    /// Unparseable lines are silently skipped (via `tracing::warn!`).
    /// `data: [DONE]` marks the stream as finished without error.
    ///
    /// Long content-delta chunks are the overwhelming majority of any
    /// stream.  A byte-level pre-filter skips JSON parsing for lines that
    /// cannot possibly carry usage, finish-reason, or model data, so the
    /// hot forwarding path stays free of per-chunk deserialization cost.
    pub fn ingest_chunk(&mut self, line: &[u8]) {
        if self.first_chunk_at.is_none() {
            self.first_chunk_at = Some(Instant::now());
        }

        // Cheap byte pre-filter: only parse JSON for lines containing one
        // of the fields we track.  A content-delta chunk like
        // `data: {"choices":[{"delta":{"content":"Hi"}}]}` matches none.
        const NEEDLES: [&[u8]; 4] = [b"usage", b"finish_reason", b"model", b"[DONE]"];
        if !NEEDLES
            .iter()
            .any(|n| memchr::memmem::find(line, n).is_some())
        {
            return;
        }

        let line_str = match std::str::from_utf8(line) {
            Ok(s) => s.trim(),
            Err(_) => {
                tracing::warn!(target: "stream_parse", "non-UTF8 chunk, skipping");
                return;
            }
        };

        let trimmed = line_str.trim();

        if trimmed.is_empty() {
            return;
        }

        if trimmed == "data: [DONE]" {
            return;
        }

        if !trimmed.starts_with("data: ") {
            return;
        }

        let json_str = &trimmed["data: ".len()..];
        let chunk: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(target: "stream_parse", error = %e, "skip unparseable chunk");
                return;
            }
        };

        if let Some(model) = chunk.get("model").and_then(|v| v.as_str()) {
            self.upstream_model = Some(model.to_owned());
        }

        if let Some(usage) = chunk.get("usage")
            && let Ok(u) = serde_json::from_value::<Usage>(usage.clone())
        {
            self.accumulated_usage = Some(u);
            self.raw_usage_json = Some(usage.clone());
        }

        if let Some(choices) = chunk.get("choices").and_then(|v| v.as_array()) {
            for choice in choices {
                if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str())
                    && fr != "null"
                {
                    self.finish_reason = Some(fr.to_owned());
                }
            }
        }

        if let Some(u) = chunk.get("usage") {
            if let Some(cached) = u
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(|v| v.as_i64())
            {
                self.cache_hit_tokens = Some(cached);
                self.cache_source = ProviderCacheKind::OpenAiPromptCache;
            }
            if let Some(r) = u
                .pointer("/completion_tokens_details/reasoning_tokens")
                .and_then(|v| v.as_i64())
            {
                self.reasoning_tokens = r;
            }
            if let Some(a) = u
                .pointer("/completion_tokens_details/audio_tokens")
                .and_then(|v| v.as_i64())
            {
                self.audio_tokens = a;
            }
        }
    }

    /// Build AuditFromProvider from accumulated stream data (ADR-010 §4).
    pub fn into_audit(&self) -> AuditFromProvider {
        AuditFromProvider {
            cache: CacheReport {
                hit_tokens: self.cache_hit_tokens,
                creation_tokens: None,
                source: self.cache_source.clone(),
            },
            reasoning_tokens: if self.reasoning_tokens > 0 {
                Some(self.reasoning_tokens)
            } else {
                None
            },
            audio_tokens: if self.audio_tokens > 0 {
                Some(self.audio_tokens)
            } else {
                None
            },
            upstream_model: self.upstream_model.clone(),
            system_fingerprint: None,
            finish_reason: self.finish_reason.clone(),
        }
    }

    /// Consume the inspector and return the accumulated [`StreamSummary`].
    pub fn finalize(self) -> StreamSummary {
        StreamSummary {
            usage: self.accumulated_usage,
            finish_reason: self.finish_reason,
            upstream_model: self.upstream_model,
            raw_usage_json: self.raw_usage_json,
        }
    }
}

impl Default for StreamInspector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ingest_line(inspector: &mut StreamInspector, line: &str) {
        inspector.ingest_chunk(line.as_bytes());
    }

    #[test]
    fn it_accumulates_usage_from_final_chunk() {
        let mut inspector = StreamInspector::new();
        ingest_line(
            &mut inspector,
            "data: {\"id\":\"1\",\"model\":\"gpt-4o\",\"choices\":[]}\n",
        );
        ingest_line(
            &mut inspector,
            "data: {\"id\":\"2\",\"model\":\"gpt-4o\",\"choices\":[{\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n",
        );
        let summary = inspector.finalize();

        let usage = summary.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
        assert_eq!(summary.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn it_skips_non_data_lines() {
        let mut inspector = StreamInspector::new();
        ingest_line(&mut inspector, ": heartbeat\n");
        ingest_line(&mut inspector, "event: message\n");
        ingest_line(&mut inspector, "\n");
        let summary = inspector.finalize();
        assert!(summary.usage.is_none());
    }

    #[test]
    fn it_handles_done_signal() {
        let mut inspector = StreamInspector::new();
        ingest_line(&mut inspector, "data: [DONE]\n");
        let summary = inspector.finalize();
        assert!(summary.usage.is_none());
    }

    #[test]
    fn it_skips_invalid_json() {
        let mut inspector = StreamInspector::new();
        ingest_line(&mut inspector, "data: not-json\n");
        let summary = inspector.finalize();
        assert!(summary.usage.is_none());
    }

    #[test]
    fn it_captures_model_from_chunks() {
        let mut inspector = StreamInspector::new();
        ingest_line(
            &mut inspector,
            "data: {\"model\":\"gpt-4o\",\"choices\":[]}\n",
        );
        let summary = inspector.finalize();
        assert_eq!(summary.upstream_model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn it_returns_none_ttft_before_any_chunk() {
        let inspector = StreamInspector::new();
        assert!(inspector.ttft_ms().is_none());
    }

    /// Regression: the byte-level pre-filter must not skip chunks that DO
    /// carry usage / finish_reason — even when embedded in a big payload.
    #[test]
    fn prefilter_still_captures_usage_and_finish_reason() {
        let mut inspector = StreamInspector::new();
        // A content-only delta with none of the needle fields: must be
        // skipped without JSON parsing (no panic, no state change).
        inspector.ingest_chunk(b"data: {\"choices\":[{\"delta\":{\"content\":\"Hi there\"}}]}");
        // A final chunk with usage and finish_reason: must be parsed.
        inspector.ingest_chunk(
            b"data: {\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}",
        );
        let summary = inspector.finalize();
        let usage = summary.usage.expect("usage chunk must be parsed");
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 3);
        assert_eq!(summary.finish_reason.as_deref(), Some("stop"));
        assert_eq!(summary.upstream_model.as_deref(), Some("gpt-4o"));
    }

    /// The word "usage" can appear inside user content; ensure the filter
    /// never *wrongly skips* a chunk that merely contains needles in text.
    /// Over-matching is fine (we just parse and discard); the test asserts
    /// parsing such a line does not corrupt state.
    #[test]
    fn prefilter_overmatch_is_harmless() {
        let mut inspector = StreamInspector::new();
        inspector
            .ingest_chunk(b"data: {\"choices\":[{\"delta\":{\"content\":\"see usage docs\"}}]}");
        let summary = inspector.finalize();
        assert!(summary.usage.is_none());
    }

    #[test]
    fn ttft_measures_from_supplied_start_instant_not_inspection_time() {
        // Regression: TTFT used to be measured from inspector construction
        // (after upstream headers arrived). For buffering upstreams that
        // flush the whole SSE body at generation end, headers and first
        // chunk arrive in the same millisecond, so ttft_ms was always 0.
        let started_at = Instant::now() - Duration::from_millis(500);
        let mut inspector = StreamInspector::new_started_at(started_at);
        inspector.ingest_chunk(b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n");
        let ttft = inspector.ttft_ms().expect("chunk ingested");
        assert!(
            ttft >= 500,
            "ttft must cover the full client wait, got {ttft}ms"
        );
    }
}

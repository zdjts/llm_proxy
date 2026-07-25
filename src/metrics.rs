//! Prometheus-compatible metrics endpoint.
//!
//! Hand-rolled (no prometheus crate) for minimal binary size.
//! Exposes counters and gauges via GET /metrics using axum.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Extension;
use axum::http::StatusCode;
use axum::response::IntoResponse;

/// Shared metrics registry.
#[derive(Clone, Default)]
pub struct Metrics {
    pub total_requests: Arc<AtomicU64>,
    pub failed_requests: Arc<AtomicU64>,
    pub stream_requests: Arc<AtomicU64>,
    pub cache_hits: Arc<AtomicU64>,
    pub active_connections: Arc<AtomicU64>,
    pub total_prompt_tokens: Arc<AtomicU64>,
    pub total_completion_tokens: Arc<AtomicU64>,
    pub retry_count: Arc<AtomicU64>,
    pub key_demotions: Arc<AtomicU64>,
    pub upstream_5xx: Arc<AtomicU64>,
    pub upstream_4xx: Arc<AtomicU64>,
    pub request_latency_sum_ms: Arc<AtomicU64>,
}

impl Metrics {
    pub fn inc_total_requests(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }
    pub fn inc_failed(&self) {
        self.failed_requests.fetch_add(1, Ordering::Relaxed);
    }
    pub fn inc_stream(&self) {
        self.stream_requests.fetch_add(1, Ordering::Relaxed);
    }
    pub fn inc_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }
    pub fn inc_active(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }
    pub fn dec_active(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }
    pub fn add_prompt_tokens(&self, n: u64) {
        self.total_prompt_tokens.fetch_add(n, Ordering::Relaxed);
    }
    pub fn add_completion_tokens(&self, n: u64) {
        self.total_completion_tokens.fetch_add(n, Ordering::Relaxed);
    }
    pub fn inc_retry(&self) {
        self.retry_count.fetch_add(1, Ordering::Relaxed);
    }
    pub fn inc_key_demotion(&self) {
        self.key_demotions.fetch_add(1, Ordering::Relaxed);
    }
    pub fn inc_upstream_5xx(&self) {
        self.upstream_5xx.fetch_add(1, Ordering::Relaxed);
    }
    pub fn inc_upstream_4xx(&self) {
        self.upstream_4xx.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_latency_ms(&self, ms: u64) {
        self.request_latency_sum_ms.fetch_add(ms, Ordering::Relaxed);
    }

    pub fn render(&self) -> String {
        let mut buf = String::with_capacity(1024);
        let _ = writeln!(buf, "# HELP llm_proxy_requests_total Total requests");
        let _ = writeln!(buf, "# TYPE llm_proxy_requests_total counter");
        let _ = writeln!(
            buf,
            "llm_proxy_requests_total {}",
            self.total_requests.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            buf,
            "# HELP llm_proxy_requests_failed_total Failed requests"
        );
        let _ = writeln!(buf, "# TYPE llm_proxy_requests_failed_total counter");
        let _ = writeln!(
            buf,
            "llm_proxy_requests_failed_total {}",
            self.failed_requests.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            buf,
            "# HELP llm_proxy_stream_requests_total Streaming requests"
        );
        let _ = writeln!(buf, "# TYPE llm_proxy_stream_requests_total counter");
        let _ = writeln!(
            buf,
            "llm_proxy_stream_requests_total {}",
            self.stream_requests.load(Ordering::Relaxed)
        );
        let _ = writeln!(buf, "# HELP llm_proxy_cache_hits_total Cache hits");
        let _ = writeln!(buf, "# TYPE llm_proxy_cache_hits_total counter");
        let _ = writeln!(
            buf,
            "llm_proxy_cache_hits_total {}",
            self.cache_hits.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            buf,
            "# HELP llm_proxy_active_connections Active connections"
        );
        let _ = writeln!(buf, "# TYPE llm_proxy_active_connections gauge");
        let _ = writeln!(
            buf,
            "llm_proxy_active_connections {}",
            self.active_connections.load(Ordering::Relaxed)
        );
        let _ = writeln!(buf, "# HELP llm_proxy_prompt_tokens_total Prompt tokens");
        let _ = writeln!(buf, "# TYPE llm_proxy_prompt_tokens_total counter");
        let _ = writeln!(
            buf,
            "llm_proxy_prompt_tokens_total {}",
            self.total_prompt_tokens.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            buf,
            "# HELP llm_proxy_completion_tokens_total Completion tokens"
        );
        let _ = writeln!(buf, "# TYPE llm_proxy_completion_tokens_total counter");
        let _ = writeln!(
            buf,
            "llm_proxy_completion_tokens_total {}",
            self.total_completion_tokens.load(Ordering::Relaxed)
        );
        let _ = writeln!(buf, "# HELP llm_proxy_retries_total Retry events");
        let _ = writeln!(buf, "# TYPE llm_proxy_retries_total counter");
        let _ = writeln!(
            buf,
            "llm_proxy_retries_total {}",
            self.retry_count.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            buf,
            "# HELP llm_proxy_key_demotions_total Key demotion events"
        );
        let _ = writeln!(buf, "# TYPE llm_proxy_key_demotions_total counter");
        let _ = writeln!(
            buf,
            "llm_proxy_key_demotions_total {}",
            self.key_demotions.load(Ordering::Relaxed)
        );
        let _ = writeln!(buf, "# HELP llm_proxy_upstream_5xx_total Upstream 5xx");
        let _ = writeln!(buf, "# TYPE llm_proxy_upstream_5xx_total counter");
        let _ = writeln!(
            buf,
            "llm_proxy_upstream_5xx_total {}",
            self.upstream_5xx.load(Ordering::Relaxed)
        );
        let _ = writeln!(buf, "# HELP llm_proxy_upstream_4xx_total Upstream 4xx");
        let _ = writeln!(buf, "# TYPE llm_proxy_upstream_4xx_total counter");
        let _ = writeln!(
            buf,
            "llm_proxy_upstream_4xx_total {}",
            self.upstream_4xx.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            buf,
            "# HELP llm_proxy_request_latency_ms_sum Total latency ms"
        );
        let _ = writeln!(buf, "# TYPE llm_proxy_request_latency_ms_sum counter");
        let _ = writeln!(
            buf,
            "llm_proxy_request_latency_ms_sum {}",
            self.request_latency_sum_ms.load(Ordering::Relaxed)
        );
        buf
    }
}

pub async fn metrics_handler(Extension(metrics): Extension<Arc<Metrics>>) -> impl IntoResponse {
    let body = metrics.render();
    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_render_produces_prometheus_format() {
        let m = Metrics::default();
        m.inc_total_requests();
        m.inc_total_requests();
        m.inc_cache_hit();
        m.add_prompt_tokens(100);

        let output = m.render();
        assert!(output.contains("llm_proxy_requests_total 2"));
        assert!(output.contains("llm_proxy_cache_hits_total 1"));
        assert!(output.contains("llm_proxy_prompt_tokens_total 100"));
        assert!(output.contains("# HELP"));
        assert!(output.contains("# TYPE"));
    }

    #[test]
    fn metrics_gauge_floats_correctly() {
        let m = Metrics::default();
        m.inc_active();
        m.inc_active();
        m.inc_active();
        m.dec_active();

        let output = m.render();
        assert!(output.contains("llm_proxy_active_connections 2"));
    }

    #[test]
    fn metrics_default_all_zero() {
        let m = Metrics::default();
        let output = m.render();
        assert!(output.contains("llm_proxy_requests_total 0"));
    }
}

//! Shared upstream HTTP client construction.
//!
//! LLM streaming and reasoning responses routinely exceed two minutes. reqwest's
//! [`reqwest::ClientBuilder::timeout`] is a **total** deadline covering connect +
//! headers + the entire body, so it aborts healthy SSE streams and long prefill.
//! Use a short connect timeout plus an idle-read timeout instead: hung TCP still
//! fails fast, but a live stream may run as long as bytes keep arriving.

use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(600);
const POOL_MAX_IDLE_PER_HOST: usize = 32;

/// Build the shared upstream `reqwest` client.
pub fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .pool_max_idle_per_host(POOL_MAX_IDLE_PER_HOST)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .user_agent("llm_proxy/2.0")
        .tcp_nodelay(true)
        .build()
        // SAFETY: rustls is compiled in; ClientBuilder::build only fails when
        // the TLS backend cannot initialise, which is process-fatal.
        .expect("reqwest ClientBuilder::build should not fail with rustls")
}

const MAX_UPSTREAM_ERROR_CHARS: usize = 512;

/// Read an upstream error response and produce a compact client-facing message.
///
/// OpenAI-compatible gateways typically return `{error:{message}}`. Relaying that
/// text is what makes a 400 actionable; a bare `upstream 400` is not.
pub async fn upstream_error_message(status: u16, response: reqwest::Response) -> String {
    let body = response.text().await.unwrap_or_default();
    format_upstream_error(status, &body)
}

/// Format an upstream HTTP error from a status code and raw body.
pub(crate) fn format_upstream_error(status: u16, body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return format!("upstream {status}");
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Some(msg) = extract_json_error_message(&json)
    {
        let msg = truncate_error_body(&msg);
        return format!("upstream {status}: {msg}");
    }
    format!("upstream {status}: {}", truncate_error_body(trimmed))
}

fn extract_json_error_message(json: &serde_json::Value) -> Option<String> {
    json.pointer("/error/message")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            json.get("message")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            json.get("error")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
}

fn truncate_error_body(body: &str) -> String {
    let collapsed: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_UPSTREAM_ERROR_CHARS {
        return collapsed;
    }
    let truncated: String = collapsed.chars().take(MAX_UPSTREAM_ERROR_CHARS).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_builds_a_client() {
        let _ = build_http_client();
    }

    #[test]
    fn timeouts_are_connect_and_idle_read_not_total() {
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(10));
        assert_eq!(READ_TIMEOUT, Duration::from_secs(600));
    }

    #[test]
    fn format_upstream_error_prefers_openai_error_message() {
        let msg = format_upstream_error(
            400,
            r#"{"error":{"message":"Invalid schema for function 'shell': schema must be a JSON Schema of 'type: \"object\"'.","type":"invalid_request_error"}}"#,
        );
        assert_eq!(
            msg,
            "upstream 400: Invalid schema for function 'shell': schema must be a JSON Schema of 'type: \"object\"'."
        );
    }

    #[test]
    fn format_upstream_error_falls_back_to_message_field() {
        let msg = format_upstream_error(400, r#"{"message":"model does not exist"}"#);
        assert_eq!(msg, "upstream 400: model does not exist");
    }

    #[test]
    fn format_upstream_error_uses_status_when_body_empty() {
        assert_eq!(format_upstream_error(400, "   "), "upstream 400");
    }

    #[test]
    fn format_upstream_error_truncates_long_plain_body() {
        let body = "x".repeat(600);
        let msg = format_upstream_error(502, &body);
        assert!(msg.starts_with("upstream 502: "));
        assert!(msg.ends_with('…'));
        assert!(msg.chars().count() < 600);
    }
}

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
}

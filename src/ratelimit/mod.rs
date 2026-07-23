//! Token-bucket rate-limiter middleware — ADR-009 §3 / T27 / T33.
//!
//! Mounted after auth via `axum::middleware::from_fn_with_state`.  Uses
//! per-client-key_hash token buckets stored in a DashMap, refilled every
//! second.  Over-limit requests receive 429 with OpenAI error format.
//! When `RateLimitConfig.enabled` is `false`, the layer is not mounted.

use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use dashmap::DashMap;
use serde_json::json;
use tokio::sync::broadcast;

use crate::alerts::AlertEvent;
use crate::auth::AuthedClient;

/// Per-client token bucket.
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    capacity: f64,
}

impl TokenBucket {
    fn new(capacity: f64) -> Self {
        Self {
            tokens: capacity,
            last_refill: Instant::now(),
            capacity,
        }
    }

    fn consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * (self.capacity / 60.0)).min(self.capacity);
            self.last_refill = now;
        }
    }
}

/// Shared rate-limiter state.
#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<DashMap<String, TokenBucket>>,
    capacity: f64,
    alert_tx: broadcast::Sender<AlertEvent>,
}

impl RateLimiter {
    pub fn new(requests_per_minute: usize, alert_tx: broadcast::Sender<AlertEvent>) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            capacity: requests_per_minute as f64,
            alert_tx,
        }
    }

    pub fn capacity(&self) -> f64 {
        self.capacity
    }
}

/// axum `from_fn_with_state` middleware.
pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let tenant_id = request
        .extensions()
        .get::<AuthedClient>()
        .map(|c| c.tenant_id.clone())
        .unwrap_or_default();

    if tenant_id.is_empty() {
        return Ok(next.run(request).await);
    }

    {
        let mut bucket = limiter
            .buckets
            .entry(tenant_id.clone())
            .or_insert_with(|| TokenBucket::new(limiter.capacity));
        if !bucket.consume() {
            let _ = limiter.alert_tx.send(AlertEvent::RateLimited {
                ts: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64,
                tenant_id: tenant_id.clone(),
            });
            let body = json!({"error":{"message":"rate limited","type":"rate_limit","code":429}});
            return Ok((StatusCode::from_u16(429).unwrap(), axum::Json(body)).into_response());
        }
    }

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::routing::get;
    use tower::util::ServiceExt;

    fn test_limiter(capacity: usize) -> Arc<RateLimiter> {
        let (tx, _) = broadcast::channel(16);
        Arc::new(RateLimiter::new(capacity, tx))
    }

    #[tokio::test]
    async fn it_allows_requests_within_capacity() {
        let limiter = test_limiter(100);
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .route_layer(axum::middleware::from_fn_with_state(
                Arc::clone(&limiter),
                rate_limit_middleware,
            ));
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn it_returns_429_when_over_capacity() {
        let limiter = test_limiter(1);
        // Test bucket logic directly: consume 2 tokens in rapid succession
        let kh = "test-key";
        let mut bucket = limiter
            .buckets
            .entry(kh.into())
            .or_insert_with(|| TokenBucket::new(limiter.capacity));
        assert!(bucket.consume(), "first request should pass");
        assert!(!bucket.consume(), "second request should be rate-limited");
    }

    #[tokio::test]
    async fn same_tenant_shares_bucket_cross_tenant_isolated() {
        let limiter = test_limiter(1);

        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .route_layer(axum::middleware::from_fn_with_state(
                Arc::clone(&limiter),
                rate_limit_middleware,
            ));

        fn req(tenant: &str) -> Request<Body> {
            let mut r = Request::builder().uri("/").body(Body::empty()).unwrap();
            r.extensions_mut().insert(crate::auth::AuthedClient {
                key_hash: format!("hash-{tenant}"),
                tenant_id: tenant.into(),
            });
            r
        }

        let resp = app.clone().oneshot(req("t-a")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app.clone().oneshot(req("t-a")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::from_u16(429).unwrap());

        let resp = app.clone().oneshot(req("t-b")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

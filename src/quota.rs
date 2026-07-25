//! Per-tenant quota tracking (Module A2 — v2.0).
//!
//! Tracks daily token usage and monthly request counts per tenant.
//! A middleware enforces limits; Prometheus gauge exposed; alerts fire at
//! 80%, 95%, and 100% thresholds.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use dashmap::DashMap;
use tokio::sync::broadcast;

use crate::alerts::AlertEvent;
use crate::auth::AuthedClient;
use crate::error::AppError;

#[derive(Debug, Clone, Default)]
pub struct QuotaConfig {
    pub enabled: bool,
    pub daily_tokens: Option<u64>,
    pub monthly_requests: Option<u64>,
}

#[derive(Debug)]
struct TenantQuota {
    daily_tokens_used: u64,
    monthly_requests_used: u64,
    day_start_epoch: u64,
    month_start_epoch: u64,
    alerted_80: bool,
    alerted_95: bool,
    alerted_100: bool,
}

impl TenantQuota {
    fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let day_start = now - (now % 86400);
        let mut month_start = now;
        #[allow(clippy::items_after_statements)]
        fn days_in_month(secs: u64) -> u64 {
            let days = secs / 86400;
            let d = days % 30;
            if d == 28 || d == 29 || d == 30 { 0 } else { d }
        }
        month_start -= days_in_month(now) * 86400;

        Self {
            daily_tokens_used: 0,
            monthly_requests_used: 0,
            day_start_epoch: day_start,
            month_start_epoch: month_start,
            alerted_80: false,
            alerted_95: false,
            alerted_100: false,
        }
    }

    fn maybe_reset(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let today_start = now - (now % 86400);
        if today_start > self.day_start_epoch {
            self.daily_tokens_used = 0;
            self.day_start_epoch = today_start;
            self.alerted_80 = false;
            self.alerted_95 = false;
            self.alerted_100 = false;
        }

        let days = now / 86400;
        let current_month_start = (days / 30) * 30 * 86400;
        if current_month_start > self.month_start_epoch {
            self.monthly_requests_used = 0;
            self.month_start_epoch = current_month_start;
        }
    }
}

#[derive(Clone)]
pub struct QuotaTracker {
    quotas: Arc<DashMap<String, TenantQuota>>,
    config: Arc<dashmap::DashMap<String, QuotaConfig>>,
    alert_tx: broadcast::Sender<AlertEvent>,
}

impl QuotaTracker {
    pub fn new(default_config: QuotaConfig, alert_tx: broadcast::Sender<AlertEvent>) -> Self {
        let configs = dashmap::DashMap::new();
        configs.insert("__default__".into(), default_config);
        Self {
            quotas: Arc::new(DashMap::new()),
            config: Arc::new(configs),
            alert_tx,
        }
    }

    pub fn set_tenant_config(&self, tenant: &str, config: QuotaConfig) {
        self.config.insert(tenant.to_owned(), config);
    }

    fn config_for(&self, tenant: &str) -> QuotaConfig {
        self.config
            .get(tenant)
            .map(|c| c.clone())
            .or_else(|| self.config.get("__default__").map(|c| c.clone()))
            .unwrap_or_default()
    }

    pub fn check_daily_tokens(&self, tenant: &str, tokens: u64) -> Result<(), AppError> {
        let config = self.config_for(tenant);
        if !config.enabled {
            return Ok(());
        }

        let Some(limit) = config.daily_tokens else {
            return Ok(());
        };

        let mut quota = self
            .quotas
            .entry(tenant.to_owned())
            .or_insert_with(TenantQuota::new);

        quota.maybe_reset();

        let used = quota.daily_tokens_used + tokens;
        let pct = (used as f64 / limit as f64) * 100.0;

        if used > limit {
            if !quota.alerted_100 {
                quota.alerted_100 = true;
                let _ = self.alert_tx.send(AlertEvent::RateLimited {
                    ts: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64,
                    tenant_id: tenant.to_owned(),
                });
            }
            return Err(AppError::Upstream {
                status: Some(429),
                retryable: true,
                bad_key_hint: false,
                msg: format!("daily token quota exceeded ({used}/{limit} tokens)"),
            });
        }

        if pct >= 95.0 && !quota.alerted_95 {
            quota.alerted_95 = true;
            let _ = self.alert_tx.send(AlertEvent::RateLimited {
                ts: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64,
                tenant_id: tenant.to_owned(),
            });
        } else if pct >= 80.0 && !quota.alerted_80 {
            quota.alerted_80 = true;
            let _ = self.alert_tx.send(AlertEvent::RateLimited {
                ts: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64,
                tenant_id: tenant.to_owned(),
            });
        }

        quota.daily_tokens_used += tokens;
        Ok(())
    }

    pub fn check_monthly_requests(&self, tenant: &str) -> Result<(), AppError> {
        let config = self.config_for(tenant);
        if !config.enabled {
            return Ok(());
        }
        let Some(limit) = config.monthly_requests else {
            return Ok(());
        };

        let mut quota = self
            .quotas
            .entry(tenant.to_owned())
            .or_insert_with(TenantQuota::new);

        quota.maybe_reset();

        if quota.monthly_requests_used >= limit {
            return Err(AppError::Upstream {
                status: Some(429),
                retryable: false,
                bad_key_hint: false,
                msg: "monthly request quota exceeded".into(),
            });
        }

        quota.monthly_requests_used += 1;
        Ok(())
    }

    pub fn usage_snapshot(&self) -> Vec<TenantQuotaSnapshot> {
        self.quotas
            .iter()
            .map(|e| {
                let tenant = e.key().clone();
                let q = e.value();
                let config = self.config_for(&tenant);
                TenantQuotaSnapshot {
                    tenant_id: tenant,
                    daily_tokens_used: q.daily_tokens_used,
                    daily_tokens_limit: config.daily_tokens,
                    monthly_requests_used: q.monthly_requests_used,
                    monthly_requests_limit: config.monthly_requests,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TenantQuotaSnapshot {
    pub tenant_id: String,
    pub daily_tokens_used: u64,
    pub daily_tokens_limit: Option<u64>,
    pub monthly_requests_used: u64,
    pub monthly_requests_limit: Option<u64>,
}

pub async fn quota_middleware(
    State(tracker): State<Arc<QuotaTracker>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let tenant_id = request
        .extensions()
        .get::<AuthedClient>()
        .map(|c| c.tenant_id.clone())
        .unwrap_or_default();

    if let Err(e) = tracker.check_monthly_requests(&tenant_id) {
        return Ok(e.into_response());
    }

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tracker() -> Arc<QuotaTracker> {
        let (tx, _) = broadcast::channel(16);
        Arc::new(QuotaTracker::new(
            QuotaConfig {
                enabled: true,
                daily_tokens: Some(1000),
                monthly_requests: Some(100),
            },
            tx,
        ))
    }

    #[test]
    fn it_allows_within_quota() {
        let tracker = test_tracker();
        assert!(tracker.check_daily_tokens("t1", 500).is_ok());
        assert!(tracker.check_monthly_requests("t1").is_ok());
    }

    #[test]
    fn it_blocks_over_daily_tokens() {
        let tracker = test_tracker();
        assert!(tracker.check_daily_tokens("t2", 2000).is_err());
    }

    #[test]
    fn it_accumulates_monthly_requests() {
        let tracker = test_tracker();
        for _ in 0..101 {
            let _ = tracker.check_monthly_requests("t3");
        }
        assert!(tracker.check_monthly_requests("t3").is_err());
    }
}

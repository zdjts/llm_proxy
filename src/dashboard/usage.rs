//! Usage analytics API — aggregated usage statistics for the dashboard.
//!
//! Serves `GET /admin/api/usage?hours=N[&tenant=...]`. All data comes from
//! `audit_hourly`; cost is estimated with the same pricing rules as the cost
//! overview so the two screens stay consistent.

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;

#[derive(Deserialize, Default)]
pub struct UsageQuery {
    pub hours: Option<i64>,
    pub tenant: Option<String>,
}

#[derive(Serialize)]
pub struct UsageSummary {
    pub requests: i64,
    pub errors: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub avg_latency_ms: i64,
    pub cost_usd: f64,
}

#[derive(Serialize)]
pub struct UsagePoint {
    /// Unix timestamp of the bucket start.
    pub ts: i64,
    /// Bucket label, e.g. "14:00".
    pub label: String,
    pub requests: i64,
    pub errors: i64,
    pub avg_latency_ms: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub cost_usd: f64,
}

#[derive(Serialize)]
pub struct UsageModelRow {
    pub model: String,
    pub requests: i64,
    pub errors: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub avg_latency_ms: i64,
    pub cost_usd: f64,
    /// 0.0–100.0, cached_tokens / prompt_tokens.
    pub cache_hit_rate: f64,
}

#[derive(Serialize)]
pub struct UsageResponse {
    pub hours: i64,
    pub today: UsageSummary,
    pub total: UsageSummary,
    pub trend: Vec<UsagePoint>,
    pub models: Vec<UsageModelRow>,
    pub tenants: Vec<String>,
    pub selected_tenant: Option<String>,
}

pub async fn admin_api_usage(
    State(state): State<crate::server::AppState>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<UsageResponse>, AppError> {
    let hours = q.hours.unwrap_or(168).clamp(1, 24 * 365);
    let tenants = super::queries::query_tenant_list(&state.db).await?;
    let pricing = state.config_store.pricing().await;
    let accounting = pricing.accounting();
    // No separate catalog fallback: `pricing` is already composed from every
    // layer (pricing_override → YAML `pricing:` → model_registry catalog),
    // so the catalog price is reachable through `accounting` itself.

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let now_hour = now / 3600;
    // `audit_hourly.hour` stores the epoch seconds of the hour bucket start.
    let day_start = (now / 86400) * 86_400;

    // ── Hourly trend (single grouped query) ──
    #[derive(sqlx::FromRow)]
    struct HourRow {
        hour: i64,
        requests: i64,
        success: i64,
        latency_ms_sum: i64,
        prompt_tokens: i64,
        completion_tokens: i64,
        cached_tokens: i64,
    }
    let trend_start = (now_hour - hours) * 3600;
    let hour_rows: Vec<HourRow> = sqlx::query_as(
        "SELECT hour, \
                SUM(request_count) AS requests, \
                SUM(success_count) AS success, \
                SUM(latency_ms_sum) AS latency_ms_sum, \
                SUM(prompt_tokens) AS prompt_tokens, \
                SUM(completion_tokens) AS completion_tokens, \
                SUM(cached_tokens) AS cached_tokens \
         FROM audit_hourly \
         WHERE hour >= ?1 \
           AND (?2 IS NULL OR tenant_id = ?2) \
         GROUP BY hour ORDER BY hour",
    )
    .bind(trend_start)
    .bind(&q.tenant)
    .fetch_all(&state.db)
    .await
    .map_err(|e| AppError::Internal(format!("usage trend query: {e}")))?;

    let mut trend: Vec<UsagePoint> = hour_rows
        .iter()
        .map(|r| UsagePoint {
            ts: r.hour,
            label: hour_label(r.hour),
            requests: r.requests,
            errors: r.requests - r.success,
            avg_latency_ms: div(r.latency_ms_sum, r.requests),
            prompt_tokens: r.prompt_tokens,
            completion_tokens: r.completion_tokens,
            cached_tokens: r.cached_tokens,
            cost_usd: 0.0,
        })
        .collect();
    // Hourly cost needs a per-hour model breakdown; fetch it in one extra query.
    let trend_costs = hourly_costs(&state.db, hours, &accounting, &q.tenant).await?;
    for point in &mut trend {
        point.cost_usd = trend_costs.get(&point.ts).copied().unwrap_or(0.0);
    }

    // ── Model breakdown for the same window ──
    let model_rows: Vec<ModelRow> = sqlx::query_as(
        "SELECT model, \
                SUM(request_count) AS requests, \
                SUM(success_count) AS success, \
                SUM(latency_ms_sum) AS latency_ms_sum, \
                SUM(prompt_tokens) AS prompt_tokens, \
                SUM(completion_tokens) AS completion_tokens, \
                SUM(cached_tokens) AS cached_tokens \
         FROM audit_hourly \
         WHERE hour >= ?1 \
           AND (?2 IS NULL OR tenant_id = ?2) \
         GROUP BY model ORDER BY requests DESC",
    )
    .bind(trend_start)
    .bind(&q.tenant)
    .fetch_all(&state.db)
    .await
    .map_err(|e| AppError::Internal(format!("usage model query: {e}")))?;

    let models: Vec<UsageModelRow> = model_rows
        .iter()
        .map(|r| {
            let cost = model_cost(
                &accounting,
                &r.model,
                q.tenant.as_deref(),
                r.prompt_tokens,
                r.completion_tokens,
            );
            UsageModelRow {
                model: r.model.clone(),
                requests: r.requests,
                errors: r.requests - r.success,
                prompt_tokens: r.prompt_tokens,
                completion_tokens: r.completion_tokens,
                cached_tokens: r.cached_tokens,
                avg_latency_ms: div(r.latency_ms_sum, r.requests),
                cost_usd: cost,
                cache_hit_rate: if r.prompt_tokens > 0 {
                    r.cached_tokens as f64 / r.prompt_tokens as f64 * 100.0
                } else {
                    0.0
                },
            }
        })
        .collect();

    // ── Window summary + today summary (reuse the same aggregation) ──
    let total = summarize(&model_rows, &accounting, q.tenant.as_deref());
    let today_rows: Vec<ModelRow> = sqlx::query_as(
        "SELECT model, \
                SUM(request_count) AS requests, \
                SUM(success_count) AS success, \
                SUM(latency_ms_sum) AS latency_ms_sum, \
                SUM(prompt_tokens) AS prompt_tokens, \
                SUM(completion_tokens) AS completion_tokens, \
                SUM(cached_tokens) AS cached_tokens \
         FROM audit_hourly \
         WHERE hour >= ?1 \
           AND (?2 IS NULL OR tenant_id = ?2) \
         GROUP BY model",
    )
    .bind(day_start)
    .bind(&q.tenant)
    .fetch_all(&state.db)
    .await
    .map_err(|e| AppError::Internal(format!("usage today query: {e}")))?;
    let today = summarize(&today_rows, &accounting, q.tenant.as_deref());

    Ok(Json(UsageResponse {
        hours,
        today,
        total,
        trend,
        models,
        tenants,
        selected_tenant: q.tenant,
    }))
}

#[derive(sqlx::FromRow)]
struct ModelRow {
    model: String,
    requests: i64,
    success: i64,
    latency_ms_sum: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
}

/// Aggregate a per-model breakdown into a window summary.
///
/// `pricing` / `catalog` / `tenant` are required because `cost_usd` is not a
/// stored column — it must be recomputed from token counts with the same
/// layered price table the request path uses.
fn summarize(
    rows: &[ModelRow],
    pricing: &crate::config::pricing::AccountingPricing<'_>,
    tenant: Option<&str>,
) -> UsageSummary {
    let requests: i64 = rows.iter().map(|r| r.requests).sum();
    let success: i64 = rows.iter().map(|r| r.success).sum();
    let latency_ms_sum: i64 = rows.iter().map(|r| r.latency_ms_sum).sum();
    let cost_usd = rows
        .iter()
        .map(|r| {
            model_cost(
                pricing,
                &r.model,
                tenant,
                r.prompt_tokens,
                r.completion_tokens,
            )
        })
        .sum();
    UsageSummary {
        requests,
        errors: requests - success,
        prompt_tokens: rows.iter().map(|r| r.prompt_tokens).sum(),
        completion_tokens: rows.iter().map(|r| r.completion_tokens).sum(),
        cached_tokens: rows.iter().map(|r| r.cached_tokens).sum(),
        avg_latency_ms: div(latency_ms_sum, requests),
        cost_usd,
    }
}

fn div(a: i64, b: i64) -> i64 {
    if b > 0 { a / b } else { 0 }
}

/// Per-model USD cost for a token pair, or 0.0 when the model is unpriced.
///
/// Since the pricing carrier is now composed from every layer (DB override →
/// YAML → catalog), this is a thin wrapper over the shared
/// [`AccountingPricing::cost_usd`] that turns "unpriced" into 0.0 for
/// summation. The old catalog fallback it replaces is gone: the catalog is
/// already layer 4 of the composed table.
fn model_cost(
    accounting: &crate::config::pricing::AccountingPricing<'_>,
    model: &str,
    tenant: Option<&str>,
    prompt_tokens: i64,
    completion_tokens: i64,
) -> f64 {
    accounting
        .cost_usd(model, tenant, prompt_tokens, completion_tokens)
        .unwrap_or(0.0)
}

fn hour_label(hour: i64) -> String {
    // `hour` is the epoch seconds of the bucket start; render as UTC HH:MM.
    let secs_of_day = hour.rem_euclid(86_400);
    format!("{:02}:{:02}", secs_of_day / 3600, (secs_of_day % 3600) / 60)
}

async fn hourly_costs(
    pool: &SqlitePool,
    hours: i64,
    accounting: &crate::config::pricing::AccountingPricing<'_>,
    tenant: &Option<String>,
) -> Result<std::collections::HashMap<i64, f64>, AppError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        hour: i64,
        model: String,
        prompt_tokens: i64,
        completion_tokens: i64,
    }
    let start = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        / 3600
        - hours)
        * 3600;
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT hour, model, SUM(prompt_tokens) AS prompt_tokens, SUM(completion_tokens) AS completion_tokens \
         FROM audit_hourly \
         WHERE hour >= ?1 AND (?2 IS NULL OR tenant_id = ?2) \
         GROUP BY hour, model",
    )
    .bind(start)
    .bind(tenant)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(format!("usage hourly cost query: {e}")))?;
    let mut map = std::collections::HashMap::new();
    for r in rows {
        let cost = model_cost(
            accounting,
            &r.model,
            tenant.as_deref(),
            r.prompt_tokens,
            r.completion_tokens,
        );
        *map.entry(r.hour).or_insert(0.0) += cost;
    }
    Ok(map)
}

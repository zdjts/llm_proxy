//! Cost overview — ADR-007 §8 (T15) + ADR-012 §2,§5 (T47,T50).
//!
//! Queries `audit_hourly` for per-model cost summary.
//! Supports `?tenant=` filter, `?format=csv`, `?format=json`.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::AppError;

#[derive(Deserialize, Default)]
pub struct CostQuery {
    pub tenant: Option<String>,
    pub format: Option<String>,
    pub hours: Option<i64>,
}

#[derive(Serialize)]
pub struct CostRow {
    pub model: String,
    pub pool_id: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub requests: i64,
    pub errors: i64,
    pub cost_usd: String,
}

#[derive(Serialize)]
pub struct CostStats {
    pub total_requests: String,
    pub total_cost: String,
    pub avg_latency: String,
    pub error_rate: String,
    pub total_errors: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
}

#[derive(Serialize)]
pub struct CostResponse {
    pub rows: Vec<CostRow>,
    pub tenants: Vec<String>,
    pub selected_tenant: Option<String>,
    pub stats: CostStats,
}

pub async fn cost_overview_handler(
    State(state): State<crate::server::AppState>,
    Query(q): Query<CostQuery>,
) -> Result<axum::response::Response, AppError> {
    let tenants = query_tenant_list(&state.db).await?;
    let pricing = state.config_store.pricing().await;
    let rows = query_cost(&state.db, &pricing, &q).await?;
    let stats = query_cost_stats(&state.db, &pricing, &q).await?;

    if q.format.as_deref() == Some("csv") {
        let mut out = String::from(
            "model,pool,prompt_tokens,completion_tokens,cached_tokens,requests,errors,cost_usd\n",
        );
        for r in &rows {
            out.push_str(&crate::dashboard::csv::csv_quote(&r.model));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.pool_id));
            out.push(',');
            out.push_str(&r.prompt_tokens.to_string());
            out.push(',');
            out.push_str(&r.completion_tokens.to_string());
            out.push(',');
            out.push_str(&r.cached_tokens.to_string());
            out.push(',');
            out.push_str(&r.requests.to_string());
            out.push(',');
            out.push_str(&r.errors.to_string());
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.cost_usd));
            out.push('\n');
        }
        return Ok((
            [
                (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"cost.csv\"",
                ),
            ],
            out,
        )
            .into_response());
    }

    Ok(Json(CostResponse {
        rows,
        tenants,
        selected_tenant: q.tenant,
        stats,
    })
    .into_response())
}

async fn query_tenant_list(pool: &SqlitePool) -> Result<Vec<String>, AppError> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT tenant_id FROM request_log ORDER BY tenant_id")
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Internal(format!("tenant list query: {e}")))?;
    Ok(rows.into_iter().map(|(t,)| t).collect())
}

async fn query_cost(
    pool: &SqlitePool,
    config: &crate::config::pricing::PricingConfig,
    q: &CostQuery,
) -> Result<Vec<CostRow>, AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let hours = q.hours.unwrap_or(24).clamp(1, 24 * 365);
    let window_start = (now / 3600 - hours) * 3600;
    let window_end = (now / 3600 + 1) * 3600;

    let rows = sqlx::query_as::<_, HourlyRow>(
        "SELECT model, pool_id, \
                SUM(request_count) AS requests, \
                SUM(success_count) AS success, \
                SUM(prompt_tokens) AS prompt_tokens, \
                SUM(completion_tokens) AS completion_tokens, \
                SUM(cached_tokens) AS cached_tokens \
         FROM audit_hourly \
         WHERE hour >= ?1 AND hour < ?2 \
           AND (?3 IS NULL OR tenant_id = ?3) \
         GROUP BY model, pool_id",
    )
    .bind(window_start)
    .bind(window_end)
    .bind(&q.tenant)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(format!("cost query: {e}")))?;

    let mut result: Vec<CostRow> = rows
        .into_iter()
        .map(|r| {
            let accounting = config.accounting();
            let price = accounting.lookup(&r.model, q.tenant.as_deref());
            let prompt_price = price.prompt / 1_000_000.0;
            let completion_price = price.completion / 1_000_000.0;
            let cost = if prompt_price > 0.0 || completion_price > 0.0 {
                let c = r.prompt_tokens as f64 * prompt_price
                    + r.completion_tokens as f64 * completion_price;
                format!("${c:.6}")
            } else {
                "?".into()
            };
            CostRow {
                model: r.model,
                pool_id: r.pool_id,
                prompt_tokens: r.prompt_tokens,
                completion_tokens: r.completion_tokens,
                cached_tokens: r.cached_tokens,
                requests: r.requests,
                errors: r.requests - r.success,
                cost_usd: cost,
            }
        })
        .collect();

    result.sort_by(|a, b| a.model.cmp(&b.model));
    Ok(result)
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct HourlyRow {
    model: String,
    pool_id: String,
    requests: i64,
    #[sqlx(rename = "success")]
    success: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
}

async fn query_cost_stats(
    pool: &SqlitePool,
    config: &crate::config::pricing::PricingConfig,
    q: &CostQuery,
) -> Result<CostStats, AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let window_start = (now / 3600 - q.hours.unwrap_or(24).clamp(1, 24 * 365)) * 3600;
    let window_end = (now / 3600 + 1) * 3600;
    #[derive(sqlx::FromRow)]
    struct StatRow {
        requests: i64,
        success: i64,
        latency_ms_sum: i64,
    }

    let row: StatRow = sqlx::query_as(
        "SELECT COALESCE(SUM(request_count),0) AS requests, \
                COALESCE(SUM(success_count),0) AS success, \
                COALESCE(SUM(latency_ms_sum),0) AS latency_ms_sum \
         FROM audit_hourly \
         WHERE hour >= ?1 AND hour < ?2 \
           AND (?3 IS NULL OR tenant_id = ?3)",
    )
    .bind(window_start)
    .bind(window_end)
    .bind(&q.tenant)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(format!("stats query: {e}")))?;

    let total_requests = row.requests;
    let total_errors = total_requests - row.success;
    let avg_latency = if total_requests > 0 {
        format!("{}ms", row.latency_ms_sum / total_requests)
    } else {
        "—".into()
    };
    let error_rate = if total_requests > 0 {
        let pct = (total_requests - row.success) as f64 / total_requests as f64 * 100.0;
        format!("{pct:.1}%")
    } else {
        "—".into()
    };

    let rows = query_cost(pool, config, q).await?;
    let total_cost = if rows.is_empty() {
        "—".into()
    } else {
        let sum: f64 = rows
            .iter()
            .map(|r| {
                let accounting = config.accounting();
                let price = accounting.lookup(&r.model, q.tenant.as_deref());
                r.prompt_tokens as f64 * price.prompt / 1_000_000.0
                    + r.completion_tokens as f64 * price.completion / 1_000_000.0
            })
            .sum();
        if sum > 0.0 {
            format!("${sum:.4}")
        } else {
            "?".into()
        }
    };

    Ok(CostStats {
        total_requests: format!("{}", total_requests),
        total_cost,
        avg_latency,
        error_rate,
        total_errors,
        prompt_tokens: rows.iter().map(|r| r.prompt_tokens).sum(),
        completion_tokens: rows.iter().map(|r| r.completion_tokens).sum(),
        cached_tokens: rows.iter().map(|r| r.cached_tokens).sum(),
    })
}

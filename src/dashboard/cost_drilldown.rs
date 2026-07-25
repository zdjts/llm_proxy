//! Cost drilldown screen — ADR-014 §3 (T63).
//!
//! JSON-only endpoint. 72h hourly breakdown per model/tenant.

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;

use crate::error::AppError;

#[derive(Deserialize)]
pub struct DrilldownQuery {
    pub model: String,
    pub tenant: Option<String>,
    pub format: Option<String>,
}

#[derive(Serialize)]
pub struct DrilldownResponse {
    pub model: String,
    pub tenant: Option<String>,
    pub stats: DrilldownStats,
    pub chart: super::traffic::ChartData,
}

#[derive(Serialize)]
pub struct DrilldownStats {
    pub cost: String,
    pub hit_rate: String,
    pub avg_latency: String,
}

fn cache_discount(source: &str) -> f64 {
    match source {
        "OpenAiPromptCache" => 0.5,
        "DeepSeekPromptCache" => 0.1,
        "AnthropicCacheControl" => 0.1,
        "GeminiCachedContent" => 0.25,
        _ => 1.0,
    }
}

pub async fn cost_drilldown_handler(
    State(state): State<crate::server::AppState>,
    Query(q): Query<DrilldownQuery>,
) -> Result<axum::response::Response, AppError> {
    let pricing = state.config.pricing.clone();
    let (stats, chart) = query_drilldown(&state.db, &pricing, &q.model, &q.tenant).await?;

    Ok(Json(DrilldownResponse {
        model: q.model,
        tenant: q.tenant,
        stats,
        chart: super::traffic::ChartData {
            lines: chart.lines,
            labels: chart.labels,
        },
    })
    .into_response())
}

async fn query_drilldown(
    pool: &SqlitePool,
    pricing: &crate::config::pricing::PricingConfig,
    model: &str,
    tenant: &Option<String>,
) -> Result<(DrilldownStats, super::traffic::ChartData), AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let window_start = (now / 3600 - 72) * 3600;
    let window_end = (now / 3600 + 1) * 3600;

    #[derive(sqlx::FromRow)]
    struct HourRow {
        hour: i64,
        requests: i64,
        success: i64,
        prompt_tokens: i64,
        completion_tokens: i64,
        cached_tokens: i64,
        latency_ms_sum: i64,
        cache_source: Option<String>,
    }

    let rows: Vec<HourRow> = sqlx::query_as(
        "SELECT hour, SUM(request_count) AS requests, SUM(success_count) AS success, \
                SUM(prompt_tokens) AS prompt_tokens, SUM(completion_tokens) AS completion_tokens, \
                SUM(cached_tokens) AS cached_tokens, SUM(latency_ms_sum) AS latency_ms_sum, \
                cache_source \
         FROM audit_hourly \
         WHERE model = ?1 AND hour >= ?2 AND hour < ?3 \
           AND (?4 IS NULL OR tenant_id = ?4) \
         GROUP BY hour, cache_source \
         ORDER BY hour",
    )
    .bind(model)
    .bind(window_start)
    .bind(window_end)
    .bind(tenant)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(format!("drilldown query: {e}")))?;

    let mut hourly: HashMap<i64, (i64, i64, i64, i64, i64, i64)> = HashMap::new();
    for r in &rows {
        let e = hourly.entry(r.hour).or_default();
        let cs = r.cache_source.as_deref().unwrap_or("None");
        let discount = cache_discount(cs);
        let billable_prompt =
            (r.prompt_tokens - r.cached_tokens) + (r.cached_tokens as f64 * discount) as i64;
        e.0 += r.requests;
        e.1 += r.success;
        e.2 += billable_prompt;
        e.3 += r.completion_tokens;
        e.4 += r.cached_tokens;
        e.5 += r.latency_ms_sum;
    }

    let mut hours: Vec<i64> = hourly.keys().copied().collect();
    hours.sort();

    let total_requests: i64 = hourly.values().map(|v| v.0).sum();
    let _total_success: i64 = hourly.values().map(|v| v.1).sum();
    let total_billable_prompt: i64 = hourly.values().map(|v| v.2).sum();
    let total_completion: i64 = hourly.values().map(|v| v.3).sum();
    let total_cached: i64 = hourly.values().map(|v| v.4).sum();
    let total_latency: i64 = hourly.values().map(|v| v.5).sum();

    let price = pricing.lookup(model, tenant.as_deref());
    let prompt_price = price.prompt;
    let completion_price = price.completion;
    let total_cost = if prompt_price > 0.0 || completion_price > 0.0 {
        let cost = total_billable_prompt as f64 * prompt_price
            + total_completion as f64 * completion_price;
        format!("${cost:.6}")
    } else {
        "?".into()
    };

    let total_prompt = total_billable_prompt + total_cached;
    let hit_rate = if total_prompt > 0 {
        format!("{:.1}%", total_cached as f64 / total_prompt as f64 * 100.0)
    } else {
        "—".into()
    };

    let avg_latency = if total_requests > 0 {
        format!("{}ms", total_latency / total_requests)
    } else {
        "—".into()
    };

    let stats = DrilldownStats {
        cost: total_cost,
        hit_rate,
        avg_latency,
    };

    // Build chart data
    let max_rq = if !hourly.is_empty() {
        hourly.values().map(|v| v.0).max().unwrap_or(1).max(1)
    } else {
        1
    };
    let max_tok = if !hourly.is_empty() {
        hourly
            .values()
            .map(|v| v.2.max(v.3))
            .max()
            .unwrap_or(1)
            .max(1)
    } else {
        1
    };

    let mut rq_points = String::from("40,180");
    let mut pr_points = String::from("40,180");
    let mut cm_points = String::from("40,180");
    let mut labels = Vec::new();

    for (i, h) in hours.iter().enumerate() {
        if let Some(v) = hourly.get(h) {
            let x = 40 + (i as i64 * 10).max(3);
            let rq_y = 180 - (v.0 as f64 / max_rq as f64 * 140.0) as i64;
            let pr_y = 180 - (v.2 as f64 / max_tok as f64 * 140.0) as i64;
            let cm_y = 180 - (v.3 as f64 / max_tok as f64 * 140.0) as i64;
            rq_points.push_str(&format!(" {x},{rq_y}"));
            pr_points.push_str(&format!(" {x},{pr_y}"));
            cm_points.push_str(&format!(" {x},{cm_y}"));
            labels.push(super::traffic::LabelData {
                x,
                text: format!("{:02}", (h / 3600) % 24),
            });
        }
    }

    let chart = super::traffic::ChartData {
        lines: vec![
            super::traffic::LineData {
                points: rq_points,
                color: "#3b82f6".into(),
            },
            super::traffic::LineData {
                points: pr_points,
                color: "#ef4444".into(),
            },
            super::traffic::LineData {
                points: cm_points,
                color: "#10b981".into(),
            },
        ],
        labels,
    };

    Ok((stats, chart))
}

//! Cost overview screen — ADR-007 §8 (T15) + ADR-012 §2,§5 (T47,T50).
//!
//! Queries `audit_hourly`, applies cache-source discount factors, and renders
//! per-model cost. Supports `?tenant=` filter and `?format=csv`.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::config::Config;
use crate::error::AppError;

use super::layout::BaseTemplate;

#[derive(Deserialize, Default)]
pub struct CostQuery {
    pub tenant: Option<String>,
    pub format: Option<String>,
}

/// Per-model upstream pricing (USD per token).
#[derive(Debug, Clone)]
pub struct ModelPrice {
    pub prompt: f64,
    pub completion: f64,
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

#[derive(Template)]
#[template(
    source = r#"<h2>成本总览</h2>
<form method="get" style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
    <select name="tenant" onchange="this.form.submit()">
        <option value="">全部 tenant</option>
        {% for t in tenants %}
        <option value="{{ t }}" {% if selected_tenant.as_deref() == Some(t.as_str()) %}selected{% endif %}>{{ t }}</option>
        {% endfor %}
    </select>
    <noscript><button type="submit">筛选</button></noscript>
</form>
<div class="stat-grid">
    <div class="stat"><div class="stat-num">{{ stats.total_requests }}</div><div class="stat-label">24h 请求</div></div>
    <div class="stat"><div class="stat-num">{{ stats.total_cost }}</div><div class="stat-label">估算成本 (USD)</div></div>
    <div class="stat"><div class="stat-num">{{ stats.avg_latency }}</div><div class="stat-label">平均延迟</div></div>
    <div class="stat"><div class="stat-num">{{ stats.error_rate }}</div><div class="stat-label">错误率</div></div>
</div>
<p class="muted">基于 audit_hourly 聚合 24h，cache 折价系数见 ADR-007 §8</p>
<table>
<thead><tr>
    <th>Model</th><th>Pool</th>
    <th class="num">总 Token</th><th class="num">缓存命中</th>
    <th class="num">计费 Prompt</th><th class="num">请求数</th><th class="num">错误数</th>
    <th class="num">估算成本 (USD)</th>
    <th></th>
</tr></thead>
<tbody>
{% for row in rows %}
<tr>
    <td>{{ row.model }}</td>
    <td>{{ row.pool_id }}</td>
    <td class="num">{{ row.total_tokens }}</td>
    <td class="num">{{ row.cached_tokens }}</td>
    <td class="num">{{ row.billable_prompt }}</td>
    <td class="num">{{ row.requests }}</td>
    <td class="num">{{ row.errors }}</td>
    <td class="num">{{ row.cost_usd }}</td>
    <td><a href="/admin/cost/drilldown?model={{ row.model }}{% if selected_tenant.is_some() %}&amp;tenant={{ selected_tenant.as_ref().unwrap() }}{% endif %}">→</a></td>
</tr>
{% endfor %}
{% if rows.is_empty() %}
<tr><td colspan="8" class="muted">— 暂无聚合数据（等待首次整点聚合）—</td></tr>
{% endif %}
</tbody>
</table>
<small class="muted">成本 = (billable_prompt * prompt_price) + (completion_tokens * completion_price)；
缓存命中按 cache_source 折价系数折算。未配置 pricing 的 model 显示 ?</small>
"#,
    ext = "html"
)]
struct CostTemplate {
    rows: Vec<CostRow>,
    tenants: Vec<String>,
    selected_tenant: Option<String>,
    stats: CostStats,
}

struct CostStats {
    total_requests: String,
    total_cost: String,
    avg_latency: String,
    error_rate: String,
}

struct CostRow {
    model: String,
    pool_id: String,
    total_tokens: i64,
    cached_tokens: i64,
    billable_prompt: i64,
    requests: i64,
    errors: i64,
    cost_usd: String,
}

/// `GET /admin` — cost overview page.
pub async fn cost_overview_handler(
    State(state): State<crate::server::AppState>,
    Query(q): Query<CostQuery>,
) -> Result<axum::response::Response, AppError> {
    let tenants = query_tenant_list(&state.db).await?;
    let rows = query_cost(&state.db, &state.config, &q).await?;
    let stats = query_cost_stats(&state.db, &state.config, &q).await?;

    if q.format.as_deref() == Some("csv") {
        let mut out = String::from(
            "model,pool,total_tokens,cached_tokens,billable_prompt,requests,errors,cost_usd\n",
        );
        for r in &rows {
            out.push_str(&super::csv::csv_quote(&r.model));
            out.push(',');
            out.push_str(&super::csv::csv_quote(&r.pool_id));
            out.push(',');
            out.push_str(&r.total_tokens.to_string());
            out.push(',');
            out.push_str(&r.cached_tokens.to_string());
            out.push(',');
            out.push_str(&r.billable_prompt.to_string());
            out.push(',');
            out.push_str(&r.requests.to_string());
            out.push(',');
            out.push_str(&r.errors.to_string());
            out.push(',');
            out.push_str(&super::csv::csv_quote(&r.cost_usd));
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

    let rendered = CostTemplate {
        rows,
        tenants,
        selected_tenant: q.tenant,
        stats,
    }
    .render()
    .map_err(|e| AppError::Internal(format!("template render: {e}")))?;
    let page = BaseTemplate {
        content: rendered,
        is_active_cost: true,
        is_active_requests: false,
        is_active_keys: false,
        is_active_traffic: false,
        is_active_alerts: false,
        is_active_help: false,
    }
    .render()
    .map_err(|e| AppError::Internal(format!("template render: {e}")))?;
    Ok(axum::response::Html(page).into_response())
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
    config: &Config,
    q: &CostQuery,
) -> Result<Vec<CostRow>, AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let window_start = (now / 3600 - 24) * 3600;
    let window_end = (now / 3600 + 1) * 3600;

    let rows = sqlx::query_as::<_, HourlyRow>(
        "SELECT model, pool_id, cache_source, \
                SUM(request_count) AS requests, \
                SUM(success_count) AS success, \
                SUM(prompt_tokens) AS prompt_tokens, \
                SUM(completion_tokens) AS completion_tokens, \
                SUM(total_tokens) AS total_tokens, \
                SUM(cached_tokens) AS cached_tokens \
         FROM audit_hourly \
         WHERE hour >= ?1 AND hour < ?2 \
           AND (?3 IS NULL OR tenant_id = ?3) \
         GROUP BY model, pool_id, cache_source",
    )
    .bind(window_start)
    .bind(window_end)
    .bind(&q.tenant)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(format!("cost query: {e}")))?;

    let mut grouped: HashMap<(String, String), CostRow> = HashMap::new();

    for r in rows {
        let entry = grouped
            .entry((r.model.clone(), r.pool_id.clone()))
            .or_insert_with(|| CostRow {
                model: r.model.clone(),
                pool_id: r.pool_id.clone(),
                total_tokens: 0,
                cached_tokens: 0,
                billable_prompt: 0,
                requests: 0,
                errors: 0,
                cost_usd: "?".into(),
            });

        entry.total_tokens += r.total_tokens;
        entry.cached_tokens += r.cached_tokens;
        entry.requests += r.requests;

        let cs = r.cache_source.as_deref().unwrap_or("None");
        let discount = cache_discount(cs);
        entry.billable_prompt +=
            ((r.prompt_tokens - r.cached_tokens) as f64 + r.cached_tokens as f64 * discount) as i64;
        entry.errors += r.requests - r.success;
    }

    for row in grouped.values_mut() {
        let price = config.pricing.lookup(&row.model, q.tenant.as_deref());
        let prompt_price = price.prompt;
        let completion_price = price.completion;
        if prompt_price > 0.0 || completion_price > 0.0 {
            let cost = row.billable_prompt as f64 * prompt_price;
            row.cost_usd = format!("${cost:.6}");
        }
    }

    let mut result: Vec<CostRow> = grouped.into_values().collect();
    result.sort_by(|a, b| a.model.cmp(&b.model));
    Ok(result)
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct HourlyRow {
    model: String,
    pool_id: String,
    cache_source: Option<String>,
    requests: i64,
    #[sqlx(rename = "success")]
    success: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
    cached_tokens: i64,
}

async fn query_cost_stats(
    pool: &SqlitePool,
    config: &Config,
    q: &CostQuery,
) -> Result<CostStats, AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let window_start = (now / 3600 - 24) * 3600;
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
            .filter_map(|r| {
                let price = config.pricing.lookup(&r.model, q.tenant.as_deref());
                if price.prompt > 0.0 {
                    Some(r.billable_prompt as f64 * price.prompt)
                } else {
                    None
                }
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
    })
}

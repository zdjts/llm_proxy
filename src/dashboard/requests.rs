//! Request detail screen — ADR-007 §9 (T16) + ADR-012 §2,§5 (T47,T50).
//!
//! Lists `request_log` rows with filter controls.  Supports `?tenant=` and
//! `?format=csv`.  SQL is fully parameterised; results capped at 100 rows.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::error::AppError;

use super::layout::BaseTemplate;

#[derive(Deserialize, Default)]
pub struct RequestFilter {
    pub tenant: Option<String>,
    pub format: Option<String>,
    pub model: Option<String>,
    pub pool_id: Option<String>,
    pub finish_reason: Option<String>,
    pub error_code: Option<String>,
    pub min_retry: Option<i32>,
    pub has_stream: Option<i32>,
    pub hours: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Template)]
#[template(
    source = r#"<h2>请求明细</h2>
<form method="get" style="display:flex;gap:8px;flex-wrap:wrap;margin-bottom:12px">
    <select name="tenant" onchange="this.form.submit()">
        <option value="">全部 tenant</option>
        {% for t in tenants %}
        <option value="{{ t }}" {% if filter.tenant.as_deref() == Some(t.as_str()) %}selected{% endif %}>{{ t }}</option>
        {% endfor %}
    </select>
    <input name="model" placeholder="model" value="{{ filter.model.clone().unwrap_or_default() }}" style="width:120px">
    <input name="pool_id" placeholder="pool_id" value="{{ filter.pool_id.clone().unwrap_or_default() }}" style="width:100px">
    <input name="finish_reason" placeholder="finish_reason" value="{{ filter.finish_reason.clone().unwrap_or_default() }}" style="width:100px">
    <input name="error_code" placeholder="error_code" value="{{ filter.error_code.clone().unwrap_or_default() }}" style="width:100px">
    <label><input type="checkbox" name="has_stream" value="1" {% if filter.has_stream == Some(1) %}checked{% endif %}> 流式</label>
    <label>重试≥<input name="min_retry" type="number" value="{{ filter.min_retry.unwrap_or(0) }}" min="0" style="width:50px"></label>
    <label>时间窗口<input name="hours" type="number" value="{{ filter.hours.unwrap_or(24) }}" min="1" max="720" style="width:50px">h</label>
    <button type="submit">筛选</button>
    <a class="btn" href="?format=csv&amp;tenant={{ filter.tenant.clone().unwrap_or_default() }}&amp;model={{ filter.model.clone().unwrap_or_default() }}&amp;pool_id={{ filter.pool_id.clone().unwrap_or_default() }}&amp;hours={{ filter.hours.unwrap_or(24) }}" style="padding:4px 8px;border:1px solid var(--brd);border-radius:4px;text-decoration:none;color:var(--fg);font-size:13px">CSV</a>
</form>
<table>
<thead><tr>
    <th>时间</th><th>Model</th><th>Pool</th><th>key_hash</th>
    <th>状态</th><th>prompt</th><th>completion</th>
    <th>cache</th><th>完成原因</th><th>错误码</th>
    <th>延迟</th><th>TTFT</th><th>重试</th>
</tr></thead>
<tbody>
{% for r in rows %}
<tr>
    <td class="muted">{{ r.ts_display }}</td>
    <td>{{ r.model }}</td>
    <td>{{ r.pool_id }}</td>
    <td class="muted" style="font-family:monospace;font-size:11px">{{ r.key_hash }}</td>
    <td>{{ r.status_code }}</td>
    <td class="num">{{ r.prompt_tokens }}</td>
    <td class="num">{{ r.completion_tokens }}</td>
    <td class="num">{{ r.cached_tokens }}</td>
    <td>{{ r.finish_reason.clone().unwrap_or_default() }}</td>
    <td class="err">{{ r.error_code.clone().unwrap_or_default() }}</td>
    <td class="num">{{ r.latency_ms }}ms</td>
    <td class="num">{{ r.ttft_ms.clone().unwrap_or_default() }}</td>
    <td class="num">{{ r.retry_count }}</td>
</tr>
{% endfor %}
{% if rows.is_empty() %}
<tr><td colspan="13" class="muted">— 无匹配记录 —</td></tr>
{% endif %}
</tbody>
</table>
<small class="muted">最近 {{ filter.hours.unwrap_or(24) }}h · 最多 100 条</small>
"#,
    ext = "html"
)]
struct RequestsTemplate {
    rows: Vec<RequestRow>,
    filter: RequestFilter,
    tenants: Vec<String>,
}

struct RequestRow {
    ts_display: String,
    model: String,
    pool_id: String,
    key_hash: String,
    status_code: String,
    prompt_tokens: String,
    completion_tokens: String,
    cached_tokens: String,
    finish_reason: Option<String>,
    error_code: Option<String>,
    latency_ms: i64,
    ttft_ms: Option<String>,
    retry_count: i32,
}

/// `GET /admin/requests` — request list with filters.
pub async fn request_list_handler(
    State(state): State<crate::server::AppState>,
    Query(filter): Query<RequestFilter>,
) -> Result<axum::response::Response, AppError> {
    let tenants = query_tenant_list(&state.db).await?;
    let rows = query_requests(&state.db, &filter).await?;

    if filter.format.as_deref() == Some("csv") {
        let mut out = String::from(
            "time,model,pool,key_hash,status,prompt,completion,cache,finish_reason,error_code,latency_ms,ttft_ms,retry\n",
        );
        for r in &rows {
            out.push_str(&super::csv::csv_quote(&r.ts_display));
            out.push(',');
            out.push_str(&super::csv::csv_quote(&r.model));
            out.push(',');
            out.push_str(&super::csv::csv_quote(&r.pool_id));
            out.push(',');
            out.push_str(&super::csv::csv_quote(&r.key_hash));
            out.push(',');
            out.push_str(&super::csv::csv_quote(&r.status_code));
            out.push(',');
            out.push_str(&super::csv::csv_quote(&r.prompt_tokens));
            out.push(',');
            out.push_str(&super::csv::csv_quote(&r.completion_tokens));
            out.push(',');
            out.push_str(&super::csv::csv_quote(&r.cached_tokens));
            out.push(',');
            out.push_str(&super::csv::csv_quote(
                &r.finish_reason.clone().unwrap_or_default(),
            ));
            out.push(',');
            out.push_str(&super::csv::csv_quote(
                &r.error_code.clone().unwrap_or_default(),
            ));
            out.push(',');
            out.push_str(&r.latency_ms.to_string());
            out.push(',');
            out.push_str(&super::csv::csv_quote(
                &r.ttft_ms.clone().unwrap_or_default(),
            ));
            out.push(',');
            out.push_str(&r.retry_count.to_string());
            out.push('\n');
        }
        return Ok((
            [
                (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"requests.csv\"",
                ),
            ],
            out,
        )
            .into_response());
    }

    let rendered = RequestsTemplate {
        rows,
        filter,
        tenants,
    }
    .render()
    .map_err(|e| AppError::Internal(format!("template: {e}")))?;
    let page = BaseTemplate {
        content: rendered,
        is_active_cost: false,
        is_active_requests: true,
        is_active_keys: false,
        is_active_traffic: false,
        is_active_alerts: false,
        is_active_help: false,
    }
    .render()
    .map_err(|e| AppError::Internal(format!("template: {e}")))?;
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

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct DbRow {
    ts: i64,
    model: String,
    pool_id: String,
    key_hash: String,
    status_code: Option<i32>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    finish_reason: Option<String>,
    error_code: Option<String>,
    latency_ms: Option<i64>,
    ttft_ms: Option<i64>,
    retry_count: Option<i32>,
}

async fn query_requests(pool: &SqlitePool, f: &RequestFilter) -> Result<Vec<RequestRow>, AppError> {
    let hours = f.hours.unwrap_or(24);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let since = now - hours * 3600 * 1000;

    let rows: Vec<DbRow> = sqlx::query_as(
        "SELECT ts, model, pool_id, key_hash, status_code, \
                prompt_tokens, completion_tokens, cached_tokens, \
                finish_reason, error_code, latency_ms, ttft_ms, retry_count \
         FROM request_log \
         WHERE ts >= ?1 \
           AND (?2 IS NULL OR model = ?2) \
           AND (?3 IS NULL OR pool_id = ?3) \
           AND (?4 IS NULL OR finish_reason = ?4) \
           AND (?5 IS NULL OR error_code = ?5) \
           AND (?6 IS NULL OR retry_count >= ?6) \
           AND (?7 IS NULL OR is_stream = ?7) \
           AND (?8 IS NULL OR tenant_id = ?8) \
         ORDER BY ts DESC \
         LIMIT 100 OFFSET ?9",
    )
    .bind(since)
    .bind(&f.model)
    .bind(&f.pool_id)
    .bind(&f.finish_reason)
    .bind(&f.error_code)
    .bind(f.min_retry)
    .bind(f.has_stream)
    .bind(&f.tenant)
    .bind(f.offset.unwrap_or(0))
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(format!("request query: {e}")))?;

    let result: Vec<RequestRow> = rows
        .into_iter()
        .map(|r| {
            let ts_display = if r.ts > 0 {
                let secs = r.ts / 1000;
                let h = (secs / 3600) % 24;
                let m = (secs / 60) % 60;
                format!("{:02}:{:02}", h, m)
            } else {
                "-".into()
            };

            RequestRow {
                ts_display,
                model: r.model,
                pool_id: r.pool_id,
                key_hash: r.key_hash,
                status_code: r.status_code.map(|s| s.to_string()).unwrap_or_default(),
                prompt_tokens: r.prompt_tokens.map(|t| t.to_string()).unwrap_or_default(),
                completion_tokens: r
                    .completion_tokens
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
                cached_tokens: r.cached_tokens.map(|t| t.to_string()).unwrap_or_default(),
                finish_reason: r.finish_reason,
                error_code: r.error_code,
                latency_ms: r.latency_ms.unwrap_or(0),
                ttft_ms: r.ttft_ms.map(|t| format!("{t}ms")),
                retry_count: r.retry_count.unwrap_or(0),
            }
        })
        .collect();

    Ok(result)
}

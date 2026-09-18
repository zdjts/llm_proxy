//! Request detail screen — ADR-007 §9 (T16) + ADR-012 §2,§5.
//!
//! Lists `request_log` rows with filter controls. JSON + CSV only.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;

use super::queries::query_tenant_list;

#[derive(Deserialize, Serialize, Default, Clone)]
pub struct RequestFilter {
    pub tenant: Option<String>,
    pub format: Option<String>,
    pub model: Option<String>,
    pub pool_id: Option<String>,
    pub finish_reason: Option<String>,
    pub error_code: Option<String>,
    pub min_retry: Option<i32>,
    pub has_stream: Option<i32>,
    /// Window in hours. `0` means all history (no time bound). Defaults to 24.
    pub hours: Option<i64>,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Serialize)]
pub struct RequestsResponse {
    pub rows: Vec<RequestRow>,
    pub filter: RequestFilter,
    pub tenants: Vec<String>,
    pub total: i64,
    pub has_more: bool,
}

#[derive(Serialize)]
pub struct RequestRow {
    pub id: String,
    pub ts: i64,
    pub model: String,
    pub pool_id: String,
    pub key_hash: String,
    pub tenant_id: String,
    pub status_code: String,
    pub prompt_tokens: String,
    pub completion_tokens: String,
    pub cached_tokens: String,
    pub finish_reason: Option<String>,
    pub error_code: Option<String>,
    pub latency_ms: i64,
    pub ttft_ms: Option<String>,
    pub retry_count: i32,
    pub is_stream: bool,
    pub cost_usd: Option<f64>,
}

#[derive(Serialize)]
pub struct RequestDetail {
    pub id: String,
    pub ts: i64,
    pub client_ip: Option<String>,
    pub model: String,
    pub pool_id: String,
    pub key_hash: String,
    pub upstream: Option<String>,
    pub status_code: Option<i32>,
    pub latency_ms: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub is_stream: bool,
    pub error: Option<String>,
    pub cached_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub cache_source: Option<String>,
    pub reasoning_tokens: Option<i64>,
    pub audio_tokens: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub upstream_model: Option<String>,
    pub system_fingerprint: Option<String>,
    pub finish_reason: Option<String>,
    pub error_code: Option<String>,
    pub retry_count: i32,
    pub tenant_id: String,
    pub user_agent: Option<String>,
    pub cost_usd: Option<f64>,
}

pub async fn request_list_handler(
    State(state): State<crate::server::AppState>,
    Query(filter): Query<RequestFilter>,
) -> Result<axum::response::Response, AppError> {
    let tenants = query_tenant_list(&state.db).await?;
    let (rows, total) = query_requests(&state.db, &filter).await?;
    let offset = filter.offset.unwrap_or(0).max(0);
    let has_more = offset + (rows.len() as i64) < total;

    if filter.format.as_deref() == Some("csv") {
        let mut out = String::from(
            "id,time,model,pool,key_hash,tenant,status,prompt,completion,cache,finish_reason,error_code,latency_ms,ttft_ms,retry,stream,cost_usd\n",
        );
        for r in &rows {
            let ts_display = format_ts(r.ts);
            out.push_str(&crate::dashboard::csv::csv_quote(&r.id));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&ts_display));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.model));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.pool_id));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.key_hash));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.tenant_id));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.status_code));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.prompt_tokens));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.completion_tokens));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.cached_tokens));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(
                &r.finish_reason.clone().unwrap_or_default(),
            ));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(
                &r.error_code.clone().unwrap_or_default(),
            ));
            out.push(',');
            out.push_str(&r.latency_ms.to_string());
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(
                &r.ttft_ms.clone().unwrap_or_default(),
            ));
            out.push(',');
            out.push_str(&r.retry_count.to_string());
            out.push(',');
            out.push_str(if r.is_stream { "1" } else { "0" });
            out.push(',');
            out.push_str(&r.cost_usd.map(|c| format!("{c:.6}")).unwrap_or_default());
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

    Ok(Json(RequestsResponse {
        rows,
        filter,
        tenants,
        total,
        has_more,
    })
    .into_response())
}

pub async fn request_detail_handler(
    State(state): State<crate::server::AppState>,
    Path(id): Path<String>,
) -> Result<Json<RequestDetail>, AppError> {
    let row = query_request_detail(&state.db, &id).await?;
    Ok(Json(row))
}

fn format_ts(ts: i64) -> String {
    if ts > 0 {
        let secs = ts / 1000;
        let h = (secs / 3600) % 24;
        let m = (secs / 60) % 60;
        let s = secs % 60;
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        "-".into()
    }
}

fn window_since(hours: Option<i64>) -> i64 {
    match hours {
        Some(0) => 0,
        Some(h) if h > 0 => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            now - h * 3600 * 1000
        }
        _ => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            now - 24 * 3600 * 1000
        }
    }
}

fn page_limit(limit: Option<i64>, csv: bool) -> i64 {
    let default = if csv { 1000 } else { 50 };
    limit.unwrap_or(default).clamp(1, 1000)
}

#[derive(sqlx::FromRow)]
struct DbRow {
    id: String,
    ts: i64,
    model: String,
    pool_id: String,
    key_hash: String,
    tenant_id: Option<String>,
    status_code: Option<i32>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    finish_reason: Option<String>,
    error_code: Option<String>,
    latency_ms: Option<i64>,
    ttft_ms: Option<i64>,
    retry_count: Option<i32>,
    is_stream: Option<i32>,
    cost_usd: Option<f64>,
}

const LIST_WHERE: &str = "ts >= ?1 \
           AND (?2 IS NULL OR model = ?2) \
           AND (?3 IS NULL OR pool_id = ?3) \
           AND (?4 IS NULL OR finish_reason = ?4) \
           AND (?5 IS NULL OR error_code = ?5) \
           AND (?6 IS NULL OR retry_count >= ?6) \
           AND (?7 IS NULL OR is_stream = ?7) \
           AND (?8 IS NULL OR tenant_id = ?8)";

async fn query_requests(
    pool: &SqlitePool,
    f: &RequestFilter,
) -> Result<(Vec<RequestRow>, i64), AppError> {
    let since = window_since(f.hours);
    let csv = f.format.as_deref() == Some("csv");
    let limit = page_limit(f.limit, csv);
    let offset = f.offset.unwrap_or(0).max(0);

    let count_sql = format!("SELECT COUNT(*) FROM request_log WHERE {LIST_WHERE}");
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(since)
        .bind(&f.model)
        .bind(&f.pool_id)
        .bind(&f.finish_reason)
        .bind(&f.error_code)
        .bind(f.min_retry)
        .bind(f.has_stream)
        .bind(&f.tenant)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Internal(format!("request count: {e}")))?;

    let list_sql = format!(
        "SELECT id, ts, model, pool_id, key_hash, tenant_id, status_code, \
                prompt_tokens, completion_tokens, cached_tokens, \
                finish_reason, error_code, latency_ms, ttft_ms, retry_count, \
                is_stream, cost_usd \
         FROM request_log \
         WHERE {LIST_WHERE} \
         ORDER BY ts DESC \
         LIMIT ?9 OFFSET ?10",
    );
    let rows: Vec<DbRow> = sqlx::query_as(&list_sql)
        .bind(since)
        .bind(&f.model)
        .bind(&f.pool_id)
        .bind(&f.finish_reason)
        .bind(&f.error_code)
        .bind(f.min_retry)
        .bind(f.has_stream)
        .bind(&f.tenant)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Internal(format!("request query: {e}")))?;

    let result: Vec<RequestRow> = rows
        .into_iter()
        .map(|r| RequestRow {
            id: r.id,
            ts: r.ts,
            model: r.model,
            pool_id: r.pool_id,
            key_hash: r.key_hash,
            tenant_id: r.tenant_id.unwrap_or_default(),
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
            is_stream: r.is_stream.unwrap_or(0) != 0,
            cost_usd: r.cost_usd,
        })
        .collect();

    Ok((result, total))
}

#[derive(sqlx::FromRow)]
struct DbDetail {
    id: String,
    ts: i64,
    client_ip: Option<String>,
    model: String,
    pool_id: String,
    key_hash: String,
    upstream: Option<String>,
    status_code: Option<i32>,
    latency_ms: Option<i64>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
    is_stream: Option<i32>,
    error: Option<String>,
    cached_tokens: Option<i64>,
    cache_creation_tokens: Option<i64>,
    cache_source: Option<String>,
    reasoning_tokens: Option<i64>,
    audio_tokens: Option<i64>,
    ttft_ms: Option<i64>,
    upstream_model: Option<String>,
    system_fingerprint: Option<String>,
    finish_reason: Option<String>,
    error_code: Option<String>,
    retry_count: Option<i32>,
    tenant_id: Option<String>,
    user_agent: Option<String>,
    cost_usd: Option<f64>,
}

async fn query_request_detail(pool: &SqlitePool, id: &str) -> Result<RequestDetail, AppError> {
    let row: Option<DbDetail> = sqlx::query_as(
        "SELECT id, ts, client_ip, model, pool_id, key_hash, upstream, \
                status_code, latency_ms, prompt_tokens, completion_tokens, \
                total_tokens, is_stream, error, cached_tokens, \
                cache_creation_tokens, cache_source, reasoning_tokens, \
                audio_tokens, ttft_ms, upstream_model, system_fingerprint, \
                finish_reason, error_code, retry_count, tenant_id, \
                user_agent, cost_usd \
         FROM request_log WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(format!("request detail: {e}")))?;

    let r = row.ok_or_else(|| AppError::NotFound(format!("request {id}")))?;
    Ok(RequestDetail {
        id: r.id,
        ts: r.ts,
        client_ip: r.client_ip,
        model: r.model,
        pool_id: r.pool_id,
        key_hash: r.key_hash,
        upstream: r.upstream,
        status_code: r.status_code,
        latency_ms: r.latency_ms,
        prompt_tokens: r.prompt_tokens,
        completion_tokens: r.completion_tokens,
        total_tokens: r.total_tokens,
        is_stream: r.is_stream.unwrap_or(0) != 0,
        error: r.error,
        cached_tokens: r.cached_tokens,
        cache_creation_tokens: r.cache_creation_tokens,
        cache_source: r.cache_source,
        reasoning_tokens: r.reasoning_tokens,
        audio_tokens: r.audio_tokens,
        ttft_ms: r.ttft_ms,
        upstream_model: r.upstream_model,
        system_fingerprint: r.system_fingerprint,
        finish_reason: r.finish_reason,
        error_code: r.error_code,
        retry_count: r.retry_count.unwrap_or(0),
        tenant_id: r.tenant_id.unwrap_or_default(),
        user_agent: r.user_agent,
        cost_usd: r.cost_usd,
    })
}

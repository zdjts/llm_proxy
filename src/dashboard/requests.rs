//! Request detail screen — ADR-007 §9 (T16) + ADR-012 §2,§5.
//!
//! Lists `request_log` rows with filter controls. JSON + CSV only.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;

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
    pub hours: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize)]
pub struct RequestsResponse {
    pub rows: Vec<RequestRow>,
    pub filter: RequestFilter,
    pub tenants: Vec<String>,
}

#[derive(Serialize)]
pub struct RequestRow {
    pub ts: i64,
    pub model: String,
    pub pool_id: String,
    pub key_hash: String,
    pub status_code: String,
    pub prompt_tokens: String,
    pub completion_tokens: String,
    pub cached_tokens: String,
    pub finish_reason: Option<String>,
    pub error_code: Option<String>,
    pub latency_ms: i64,
    pub ttft_ms: Option<String>,
    pub retry_count: i32,
}

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
            let ts_display = if r.ts > 0 {
                let secs = r.ts / 1000;
                let h = (secs / 3600) % 24;
                let m = (secs / 60) % 60;
                let s = secs % 60;
                format!("{h:02}:{m:02}:{s:02}")
            } else {
                "-".into()
            };
            out.push_str(&crate::dashboard::csv::csv_quote(&ts_display));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.model));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.pool_id));
            out.push(',');
            out.push_str(&crate::dashboard::csv::csv_quote(&r.key_hash));
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
        .map(|r| RequestRow {
            ts: r.ts,
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
        })
        .collect();

    Ok(result)
}

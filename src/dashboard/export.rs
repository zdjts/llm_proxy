//! Structured log export (Module D2 — v2.0).
//!
//! `GET /admin/export?format=jsonl` exports recent request logs in
//! JSON Lines or plain JSON format. Parquet export is a stub.

use axum::extract::{Query, State};
use axum::http::header;
use axum::response::Response;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "default_hours")]
    pub hours: u32,
}

fn default_format() -> String {
    "jsonl".into()
}

fn default_hours() -> u32 {
    24
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct ExportRow {
    id: String,
    ts: i64,
    client_ip: Option<String>,
    model: String,
    pool_id: String,
    key_hash: String,
    status_code: Option<i16>,
    latency_ms: Option<i64>,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    total_tokens: Option<i32>,
    is_stream: i32,
    error_code: Option<String>,
    tenant_id: Option<String>,
    cost_usd: Option<f64>,
}

impl ExportRow {
    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "ts": self.ts,
            "client_ip": self.client_ip,
            "model": self.model,
            "pool_id": self.pool_id,
            "key_hash": self.key_hash,
            "status_code": self.status_code,
            "latency_ms": self.latency_ms,
            "prompt_tokens": self.prompt_tokens,
            "completion_tokens": self.completion_tokens,
            "total_tokens": self.total_tokens,
            "is_stream": self.is_stream != 0,
            "error_code": self.error_code,
            "tenant_id": self.tenant_id,
            "cost_usd": self.cost_usd,
        })
    }
}

pub async fn export_handler(
    State(state): State<crate::server::AppState>,
    Query(params): Query<ExportQuery>,
) -> Result<Response, AppError> {
    let cutoff = chrono_now_millis() - (params.hours as i64) * 3600 * 1000;

    let rows: Vec<ExportRow> = sqlx::query_as(
        r#"
        SELECT
            id, ts, client_ip, model, pool_id, key_hash,
            status_code, latency_ms, prompt_tokens, completion_tokens,
            total_tokens, is_stream, error_code,
            tenant_id, cost_usd
        FROM request_log
        WHERE ts > ?1
        ORDER BY ts DESC
        LIMIT 10000
        "#,
    )
    .bind(cutoff)
    .fetch_all(&state.db)
    .await
    .map_err(|e| AppError::Internal(format!("export query failed: {e}")))?;

    match params.format.as_str() {
        "jsonl" => {
            let mut body = String::new();
            for row in &rows {
                let line = serde_json::to_string(&row.to_json_value()).unwrap_or_default();
                body.push_str(&line);
                body.push('\n');
            }
            Ok(Response::builder()
                .header(header::CONTENT_TYPE, "application/x-ndjson")
                .header(
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=export.jsonl",
                )
                .body(axum::body::Body::from(body))
                .unwrap())
        }
        "json" => {
            let values: Vec<serde_json::Value> =
                rows.iter().map(|r| r.to_json_value()).collect();
            let body = serde_json::to_string_pretty(&values).unwrap_or_default();
            Ok(Response::builder()
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=export.json",
                )
                .body(axum::body::Body::from(body))
                .unwrap())
        }
        "parquet" => Ok(Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({"error":"parquet export requires additional native dependencies"}).to_string(),
            ))
            .unwrap()),
        _ => Err(AppError::BadRequest(format!(
            "unsupported export format '{}'. use jsonl, json, or parquet",
            params.format
        ))),
    }
}

fn chrono_now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

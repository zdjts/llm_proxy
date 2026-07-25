//! Traffic trend screen — ADR-007 §11 (T18) + ADR-012 §2,§5.
//!
//! JSON + CSV endpoint. Queries `audit_hourly` grouped by day.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::AppError;

#[derive(Deserialize, Default)]
pub struct TrendQuery {
    pub days: Option<i64>,
    pub tenant: Option<String>,
    pub format: Option<String>,
}

#[derive(Serialize)]
pub struct TrafficResponse {
    pub chart: ChartData,
    pub days: i64,
    pub w: i64,
    pub tenants: Vec<String>,
    pub selected_tenant: Option<String>,
}

#[derive(Serialize)]
pub struct ChartData {
    pub lines: Vec<LineData>,
    pub labels: Vec<LabelData>,
}

#[derive(Serialize)]
pub struct LineData {
    pub points: String,
    pub color: String,
}

#[derive(Serialize)]
pub struct LabelData {
    pub x: i64,
    pub text: String,
}

pub async fn traffic_trend_handler(
    State(state): State<crate::server::AppState>,
    Query(q): Query<TrendQuery>,
) -> Result<axum::response::Response, AppError> {
    let tenants = query_tenant_list(&state.db).await?;
    let days = q.days.unwrap_or(7).clamp(1, 90);
    let chart = query_traffic(&state.db, days, &q.tenant).await?;
    let w = 40 + days.max(7) * 50;

    if q.format.as_deref() == Some("csv") {
        let mut out = String::from("day,requests,avg_latency_ms\n");
        let raw = query_traffic_raw(&state.db, days, &q.tenant).await?;
        for r in &raw {
            let avg_lat = if r.cnt > 0 { r.lat_sum / r.cnt } else { 0 };
            out.push_str(&r.day.to_string());
            out.push(',');
            out.push_str(&r.rqs.to_string());
            out.push(',');
            out.push_str(&avg_lat.to_string());
            out.push('\n');
        }
        return Ok((
            [
                (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"traffic.csv\"",
                ),
            ],
            out,
        )
            .into_response());
    }

    Ok(Json(TrafficResponse {
        chart,
        days,
        w,
        tenants,
        selected_tenant: q.tenant,
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

async fn query_traffic(
    pool: &SqlitePool,
    days: i64,
    tenant: &Option<String>,
) -> Result<ChartData, AppError> {
    let rows = query_traffic_raw(pool, days, tenant).await?;

    if rows.is_empty() {
        return Ok(ChartData {
            lines: vec![],
            labels: vec![],
        });
    }

    let max_rqs = rows.iter().map(|r| r.rqs).max().unwrap_or(1).max(1);
    let max_lat = rows
        .iter()
        .map(|r| if r.cnt > 0 { r.lat_sum / r.cnt } else { 0 })
        .max()
        .unwrap_or(1)
        .max(1);

    let mut rqs_points = String::from("40,200");
    let mut lat_points = String::from("40,200");
    let mut labels = Vec::new();

    for (i, r) in rows.iter().enumerate() {
        let x = 40 + (i as i64) * (50.min(days).max(3));
        let avg_lat = if r.cnt > 0 { r.lat_sum / r.cnt } else { 0 };
        let rq_y = 200 - (r.rqs as f64 / max_rqs as f64 * 160.0) as i64;
        let lat_y = 200 - (avg_lat as f64 / max_lat as f64 * 160.0) as i64;

        rqs_points.push_str(&format!(" {x},{rq_y}"));
        lat_points.push_str(&format!(" {x},{lat_y}"));

        let day_label = if r.day > 0 {
            let d = (r.day / 86400) % 31 + 1;
            format!("{d}日")
        } else {
            "?".into()
        };

        labels.push(LabelData { x, text: day_label });
    }

    Ok(ChartData {
        lines: vec![
            LineData {
                points: rqs_points,
                color: "#2563eb".into(),
            },
            LineData {
                points: lat_points,
                color: "#dc2626".into(),
            },
        ],
        labels,
    })
}

async fn query_traffic_raw(
    pool: &SqlitePool,
    days: i64,
    tenant: &Option<String>,
) -> Result<Vec<DayRow>, AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let start = (now / 3600 - days * 24) * 3600;

    let rows: Vec<DayRow> = sqlx::query_as(
        "SELECT (hour / 86400) * 86400 AS day, \
                SUM(request_count) AS rqs, \
                SUM(latency_ms_sum) AS lat_sum, \
                SUM(request_count) AS cnt \
         FROM audit_hourly \
         WHERE hour >= ?1 \
           AND (?2 IS NULL OR tenant_id = ?2) \
         GROUP BY day \
         ORDER BY day",
    )
    .bind(start)
    .bind(tenant)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(format!("traffic query: {e}")))?;

    Ok(rows)
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct DayRow {
    day: i64,
    rqs: i64,
    lat_sum: i64,
    cnt: i64,
}

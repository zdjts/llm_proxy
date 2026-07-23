//! Traffic trend screen — ADR-007 §11 (T18) + ADR-012 §2,§5 (T47,T50).
//!
//! Queries `audit_hourly` grouped by day and renders a native SVG chart.
//! Supports `?tenant=`, `?format=csv`, and `?days=7|30`.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::error::AppError;

use super::layout::BaseTemplate;

#[derive(Deserialize, Default)]
pub struct TrendQuery {
    pub days: Option<i64>,
    pub tenant: Option<String>,
    pub format: Option<String>,
}

#[derive(Template)]
#[template(
    source = r#"<h2>调用量趋势</h2>
<form method="get" style="display:flex;gap:8px;align-items:center;margin-bottom:4px">
    <select name="tenant" onchange="this.form.submit()">
        <option value="">全部 tenant</option>
        {% for t in tenants %}
        <option value="{{ t }}" {% if selected_tenant.as_deref() == Some(t.as_str()) %}selected{% endif %}>{{ t }}</option>
        {% endfor %}
    </select>
</form>
<p class="muted">
    <a href="?days=7{% if selected_tenant.is_some() %}&amp;tenant={{ selected_tenant.as_ref().unwrap() }}{% endif %}" {% if days == 7 %}style="font-weight:bold"{% endif %}>7天</a> ·
    <a href="?days=30{% if selected_tenant.is_some() %}&amp;tenant={{ selected_tenant.as_ref().unwrap() }}{% endif %}" {% if days == 30 %}style="font-weight:bold"{% endif %}>30天</a>
</p>
<svg width="100%" height="240" viewBox="0 0 {{ w }} 240" style="background:var(--card);border:1px solid var(--brd);border-radius:4px">
    <line x1="40" y1="200" x2="{{ w }}" y2="200" stroke="var(--brd)" stroke-width="1"/>
    <line x1="40" y1="40" x2="40" y2="200" stroke="var(--brd)" stroke-width="1"/>
    {% for line in chart.lines %}<polyline points="{{ line.points }}" fill="none" stroke="{{ line.color }}" stroke-width="2"/>{% endfor %}
    {% for label in chart.labels %}<text x="{{ label.x }}" y="216" fill="var(--muted)" font-size="10" text-anchor="end" transform="rotate(-25,{{ label.x }},216)">{{ label.text }}</text>{% endfor %}
</svg>
<small class="muted">蓝色 = 请求数 · 红色 = 平均延迟(ms)</small>
"#,
    ext = "html"
)]
struct TrafficTemplate {
    chart: ChartData,
    days: i64,
    w: i64,
    tenants: Vec<String>,
    selected_tenant: Option<String>,
}

struct ChartData {
    lines: Vec<LineData>,
    labels: Vec<LabelData>,
}

struct LineData {
    points: String,
    color: String,
}

struct LabelData {
    x: i64,
    text: String,
}

/// `GET /admin/traffic` — traffic trend chart.
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
        // Re-run query at raw resolution
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

    let rendered = TrafficTemplate {
        chart,
        days,
        w,
        tenants,
        selected_tenant: q.tenant,
    }
    .render()
    .map_err(|e| AppError::Internal(format!("template: {e}")))?;
    let page = BaseTemplate {
        content: rendered,
        is_active_cost: false,
        is_active_requests: false,
        is_active_keys: false,
        is_active_traffic: true,
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
            let secs = r.day;
            let d = (secs / 86400) % 31 + 1;
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

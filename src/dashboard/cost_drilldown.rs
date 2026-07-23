//! Per-model per-tenant cost drilldown screen — ADR-014 §4 (T61).
//!
//! `GET /admin/cost/drilldown?model=<m>&tenant=<t>`
//! Renders 3 SVG trend lines + 3 summary stat cards.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::error::AppError;

use super::layout::BaseTemplate;

#[derive(Deserialize)]
pub struct DrilldownQuery {
    pub model: String,
    pub tenant: Option<String>,
}

#[derive(Template)]
#[template(
    source = r#"<h2>{{ model }} — 成本下钻</h2>
<p class="muted">
    {% if tenant.is_some() %}Tenant: {{ tenant.as_ref().unwrap() }} · {% endif %}
    <a href="/admin">← 返回成本总览</a>
</p>

<div style="display:flex;gap:16px;margin-bottom:16px">
    <div class="card" style="flex:1;text-align:center">
        <div class="muted" style="font-size:12px">估算成本 (USD)</div>
        <div style="font-size:24px;font-weight:700">{{ stats.cost }}</div>
    </div>
    <div class="card" style="flex:1;text-align:center">
        <div class="muted" style="font-size:12px">缓存命中率</div>
        <div style="font-size:24px;font-weight:700">{{ stats.hit_rate }}</div>
    </div>
    <div class="card" style="flex:1;text-align:center">
        <div class="muted" style="font-size:12px">平均延迟</div>
        <div style="font-size:24px;font-weight:700">{{ stats.avg_latency }}</div>
    </div>
</div>

<svg width="100%" height="260" viewBox="0 0 {{ w }} 260" style="background:var(--card);border:1px solid var(--brd);border-radius:4px">
    <line x1="50" y1="220" x2="{{ w }}" y2="220" stroke="var(--brd)" stroke-width="1"/>
    <line x1="50" y1="30" x2="50" y2="220" stroke="var(--brd)" stroke-width="1"/>
    {% for line in chart.lines %}<polyline points="{{ line.points }}" fill="none" stroke="{{ line.color }}" stroke-width="2"/>{% endfor %}
    {% for label in chart.labels %}<text x="{{ label.x }}" y="236" fill="var(--muted)" font-size="9" text-anchor="end" transform="rotate(-30,{{ label.x }},236)">{{ label.text }}</text>{% endfor %}
</svg>
<small class="muted">蓝=请求量 · 红=折后Prompt Tokens · 绿=完成Tokens</small>
"#,
    ext = "html"
)]
struct DrilldownTemplate {
    model: String,
    tenant: Option<String>,
    chart: ChartData,
    stats: DrilldownStats,
    w: i64,
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

struct DrilldownStats {
    cost: String,
    hit_rate: String,
    avg_latency: String,
}

struct HourRow {
    hour: i64,
    rqs: i64,
    pt: i64,
    ct: i64,
    cpt: i64,
}

/// `GET /admin/cost/drilldown?model=&tenant=`
pub async fn cost_drilldown_handler(
    State(state): State<crate::server::AppState>,
    Query(q): Query<DrilldownQuery>,
) -> Result<impl IntoResponse, AppError> {
    if q.model.is_empty() {
        return Err(AppError::BadRequest("model is required".into()));
    }

    let rows = query_model_hourly(&state.db, &q.model, &q.tenant).await?;
    let stats = compute_stats(&state.config, &q.model, q.tenant.as_deref(), &rows);
    let chart = build_chart(&rows);

    let w = 50 + rows.len().max(12) as i64 * 14;

    let rendered = DrilldownTemplate {
        model: q.model,
        tenant: q.tenant,
        chart,
        stats,
        w,
    }
    .render()
    .map_err(|e| AppError::Internal(format!("template: {e}")))?;

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
    .map_err(|e| AppError::Internal(format!("template: {e}")))?;
    Ok(axum::response::Html(page).into_response())
}

async fn query_model_hourly(
    pool: &SqlitePool,
    model: &str,
    tenant: &Option<String>,
) -> Result<Vec<HourRow>, AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let start = (now / 3600 - 72) * 3600;

    let rows: Vec<(i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT hour, \
                SUM(request_count) AS rqs, \
                SUM(prompt_tokens) AS pt, \
                SUM(cached_tokens) AS ct, \
                SUM(completion_tokens) AS cpt \
         FROM audit_hourly \
         WHERE hour >= ?1 \
           AND model = ?2 \
           AND (?3 IS NULL OR tenant_id = ?3) \
         GROUP BY hour \
         ORDER BY hour",
    )
    .bind(start)
    .bind(model)
    .bind(tenant)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(format!("drilldown query: {e}")))?;

    Ok(rows
        .into_iter()
        .map(|(hour, rqs, pt, ct, cpt)| HourRow {
            hour,
            rqs,
            pt,
            ct,
            cpt,
        })
        .collect())
}

fn compute_stats(
    config: &crate::config::Config,
    model: &str,
    tenant: Option<&str>,
    rows: &[HourRow],
) -> DrilldownStats {
    let price = config.pricing.lookup(model, tenant);
    let total_pt: i64 = rows.iter().map(|r| r.pt).sum();
    let total_ct: i64 = rows.iter().map(|r| r.ct).sum();

    let cost = if price.prompt > 0.0 {
        let billable = (total_pt - total_ct) as f64 + total_ct as f64 * 0.5;
        format!("${:.6}", billable * price.prompt)
    } else {
        "?".into()
    };

    let hit_rate = if total_pt > 0 {
        format!("{:.1}%", total_ct as f64 / total_pt as f64 * 100.0)
    } else {
        "—".into()
    };

    let avg_latency = "—";

    DrilldownStats {
        cost,
        hit_rate,
        avg_latency: avg_latency.into(),
    }
}

fn build_chart(rows: &[HourRow]) -> ChartData {
    if rows.is_empty() {
        return ChartData {
            lines: vec![],
            labels: vec![],
        };
    }

    let max_rqs = rows.iter().map(|r| r.rqs).max().unwrap_or(1).max(1);
    let max_pt = rows.iter().map(|r| r.pt).max().unwrap_or(1).max(1);
    let max_cpt = rows.iter().map(|r| r.cpt).max().unwrap_or(1).max(1);

    let mut rqs_points = String::from("50,220");
    let mut pt_points = String::from("50,220");
    let mut cpt_points = String::from("50,220");
    let mut labels = Vec::new();

    for (i, r) in rows.iter().enumerate() {
        let x = 50 + i as i64 * 14;
        let rq_y = 220 - (r.rqs as f64 / max_rqs as f64 * 190.0) as i64;
        let pt_y = 220 - (r.pt as f64 / max_pt as f64 * 190.0) as i64;
        let cpt_y = 220 - (r.cpt as f64 / max_cpt as f64 * 190.0) as i64;

        rqs_points.push_str(&format!(" {x},{rq_y}"));
        pt_points.push_str(&format!(" {x},{pt_y}"));
        cpt_points.push_str(&format!(" {x},{cpt_y}"));

        if i % 6 == 0 {
            let secs = r.hour;
            let h = (secs / 3600) % 24;
            labels.push(LabelData {
                x,
                text: format!("{h:02}h"),
            });
        }
    }

    ChartData {
        lines: vec![
            LineData {
                points: rqs_points,
                color: "#2563eb".into(),
            },
            LineData {
                points: pt_points,
                color: "#dc2626".into(),
            },
            LineData {
                points: cpt_points,
                color: "#16a34a".into(),
            },
        ],
        labels,
    }
}

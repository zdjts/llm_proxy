//! Key health / failover screen — ADR-007 §10 (T17).
//!
//! Reads in-memory state from `Router::pool_snapshot()` and `BadKeyRegistry`;
//! enriches with 7‑day success-rate buckets from `audit_hourly`.

use askama::Template;
use axum::extract::State;
use axum::response::IntoResponse;
use sqlx::SqlitePool;

use crate::error::AppError;

use super::layout::BaseTemplate;

#[derive(Template)]
#[template(
    source = r#"<h2>Key 健康 / Failover</h2>
{% for pool in pools %}
<div class="card">
<h3>Pool: {{ pool.pool_id }}</h3>
<table>
<thead><tr>
    <th>key_hash</th><th>权重</th><th>状态</th><th>7d 成功率</th>
</tr></thead>
<tbody>
{% for key in pool.keys %}
<tr>
    <td style="font-family:monospace;font-size:11px">{{ key.key_hash }}</td>
    <td class="num">{{ key.weight }}</td>
    <td>{% if key.healthy %}<span style="color:var(--accent)">● 健康</span>{% else %}<span class="err">● 已剔除</span>{% endif %}</td>
    <td>
        {% if key.sparkline.len() > 0 %}
        <div class="spark">{% for h in key.sparkline %}<div class="spark-bar" style="height:{{ h }}px"></div>{% endfor %}</div>
        <small class="muted">{{ key.success_rate }}</small>
        {% else %}
        <span class="muted">—</span>
        {% endif %}
    </td>
</tr>
{% endfor %}
</tbody>
</table>
</div>
{% endfor %}
{% if pools.is_empty() %}
<div class="card"><p class="muted">— 无已注册的 key pool —</p></div>
{% endif %}
"#,
    ext = "html"
)]
struct KeysTemplate {
    pools: Vec<PoolView>,
}

struct PoolView {
    pool_id: String,
    keys: Vec<KeyView>,
}

struct KeyView {
    key_hash: String,
    weight: u32,
    healthy: bool,
    sparkline: Vec<u32>,
    success_rate: String,
}

/// `GET /admin/keys` — key health dashboard.
pub async fn key_health_handler(
    State(state): State<crate::server::AppState>,
) -> Result<impl IntoResponse, AppError> {
    let snaps = state.router.pool_snapshot();
    let mut pools = Vec::new();

    for snap in snaps {
        let mut keys = Vec::new();
        for ks in snap.keys {
            let (spark, rate) = key_success_rate(&state.db, &ks.key_hash).await;
            keys.push(KeyView {
                key_hash: ks.key_hash,
                weight: ks.weight,
                healthy: ks.healthy,
                sparkline: spark,
                success_rate: rate,
            });
        }
        pools.push(PoolView {
            pool_id: snap.pool_id,
            keys,
        });
    }

    let rendered = KeysTemplate { pools }
        .render()
        .map_err(|e| AppError::Internal(format!("template: {e}")))?;
    let page = BaseTemplate {
        content: rendered,
        is_active_cost: false,
        is_active_requests: false,
        is_active_keys: true,
        is_active_traffic: false,
        is_active_alerts: false,
        is_active_help: false,
    }
    .render()
    .map_err(|e| AppError::Internal(format!("template: {e}")))?;
    Ok(axum::response::Html(page))
}

/// Query 7-day success-rate buckets for a single key_hash.
/// Returns 8 day-grouped sparkline heights (0..20) and a label like "99.8%".
async fn key_success_rate(pool: &SqlitePool, key_hash: &str) -> (Vec<u32>, String) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let window_start = (now / 3600 - 7 * 24) * 3600;

    let rows: Result<Vec<DayBucket>, _> = sqlx::query_as(
        "SELECT (hour / 86400) * 86400 AS day, \
                SUM(success_count) AS success, \
                SUM(request_count) AS total \
         FROM audit_hourly \
         WHERE key_hash = ?1 AND hour >= ?2 \
         GROUP BY day \
         ORDER BY day",
    )
    .bind(key_hash)
    .bind(window_start)
    .fetch_all(pool)
    .await;

    let rows = match rows {
        Ok(r) => r,
        Err(_) => return (vec![], "—".into()),
    };

    let max_total = rows.iter().map(|r| r.total).max().unwrap_or(1).max(1);
    let sparkline: Vec<u32> = rows
        .iter()
        .map(|r| ((r.success as f64 / max_total as f64) * 20.0) as u32)
        .collect();

    let total_success: i64 = rows.iter().map(|r| r.success).sum();
    let total_all: i64 = rows.iter().map(|r| r.total).sum();
    let rate = if total_all > 0 {
        format!("{:.1}%", total_success as f64 / total_all as f64 * 100.0)
    } else {
        "—".into()
    };

    (sparkline, rate)
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct DayBucket {
    #[allow(dead_code)]
    day: i64,
    success: i64,
    total: i64,
}

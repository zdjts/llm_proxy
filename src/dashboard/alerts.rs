//! Alerts event stream screen — ADR-012 §3 (T48) + ADR-013 §2.3 (T54).

use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::error::AppError;

use super::layout::BaseTemplate;

#[derive(Deserialize, Default)]
pub struct AlertFilter {
    pub r#type: Option<String>,
    pub tenant: Option<String>,
    pub ts_from: Option<i64>,
    pub ts_to: Option<i64>,
}

#[derive(Template)]
#[template(
    source = r#"<h2>告警事件流</h2>
<form method="get" style="display:flex;gap:8px;flex-wrap:wrap;margin-bottom:12px">
    <select name="type">{{ type_select|safe }}</select>
    <input name="tenant" placeholder="tenant_id" value="{{ tenant_filter }}" style="width:100px">
    <input name="ts_from" placeholder="ts_from (unix)" value="{{ ts_from_filter }}" style="width:120px">
    <input name="ts_to" placeholder="ts_to (unix)" value="{{ ts_to_filter }}" style="width:120px">
    <button type="submit">筛选</button>
</form>
<p class="muted">{{ event_count }} 条记录（最近 100）</p>
<table>
<thead><tr>
    <th>ID</th><th>时间</th><th>类型</th><th>Pool</th><th>Tenant</th>
    <th>Model</th><th>错误码</th><th>消息</th>
</tr></thead>
<tbody>
{% for e in events %}
<tr>
    <td class="muted">{{ e.id }}</td>
    <td class="muted">{{ e.ts_display }}</td>
    <td>{{ e.event_type }}</td>
    <td>{{ e.pool_id }}</td>
    <td>{{ e.tenant_id }}</td>
    <td>{{ e.model }}</td>
    <td>{{ e.error_code }}</td>
    <td style="max-width:260px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ e.msg }}</td>
</tr>
{% endfor %}
{% if events.is_empty() %}
<tr><td colspan="8" class="muted">— 暂无告警事件 —</td></tr>
{% endif %}
</tbody>
</table>
<small class="muted">数据来源于 alert_event SQLite 表</small>
"#,
    ext = "html"
)]
struct AlertsTemplate {
    events: Vec<AlertDisplayRow>,
    type_select: String,
    tenant_filter: String,
    ts_from_filter: String,
    ts_to_filter: String,
    event_count: usize,
}

struct AlertDisplayRow {
    id: i64,
    ts_display: String,
    event_type: String,
    pool_id: String,
    tenant_id: String,
    model: String,
    error_code: String,
    msg: String,
}

/// `GET /admin/alerts` — recent alert events from SQLite.
pub async fn alerts_handler(
    State(state): State<crate::server::AppState>,
    Query(filter): Query<AlertFilter>,
) -> Result<impl IntoResponse, AppError> {
    let event_types = [
        "UpstreamError",
        "LatencySpike",
        "RateLimited",
        "PoolExhausted",
    ];
    let selected = filter.r#type.as_deref().unwrap_or("");
    let mut type_select = String::from("<option value=\"\">全部类型</option>");
    for t in &event_types {
        let sel = if selected == *t { " selected" } else { "" };
        type_select.push_str(&format!("<option value=\"{t}\"{sel}>{t}</option>"));
    }

    let tenant_filter = filter.tenant.clone().unwrap_or_default();
    let ts_from_filter = filter.ts_from.map(|v| v.to_string()).unwrap_or_default();
    let ts_to_filter = filter.ts_to.map(|v| v.to_string()).unwrap_or_default();

    let rows = crate::alerts::db::query_alert_events(
        &state.db,
        100,
        &filter.r#type,
        &filter.tenant,
        &filter.ts_from,
        &filter.ts_to,
    )
    .await
    .unwrap_or_default();

    let event_count = rows.len();
    let events: Vec<AlertDisplayRow> = rows
        .into_iter()
        .map(|r| {
            let ts_display = if r.ts > 0 {
                let secs = r.ts;
                let h = (secs / 3600) % 24;
                let m = (secs / 60) % 60;
                format!("{:02}:{:02}", h, m)
            } else {
                "-".into()
            };
            AlertDisplayRow {
                id: r.id,
                ts_display,
                event_type: r.r#type,
                pool_id: r.pool_id.unwrap_or_default(),
                tenant_id: r.tenant_id.unwrap_or_default(),
                model: r.model.unwrap_or_default(),
                error_code: r.error_code.unwrap_or_default(),
                msg: r.msg.unwrap_or_default(),
            }
        })
        .collect();

    let rendered = AlertsTemplate {
        events,
        type_select,
        tenant_filter,
        ts_from_filter,
        ts_to_filter,
        event_count,
    }
    .render()
    .map_err(|e| AppError::Internal(format!("template render: {e}")))?;
    let page = BaseTemplate {
        content: rendered,
        is_active_cost: false,
        is_active_requests: false,
        is_active_keys: false,
        is_active_traffic: false,
        is_active_alerts: true,
        is_active_help: false,
    }
    .render()
    .map_err(|e| AppError::Internal(format!("template render: {e}")))?;
    Ok(axum::response::Html(page).into_response())
}

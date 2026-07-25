//! Alerts event stream screen — ADR-012 §3 (T48) + ADR-013 §2.3 (T54).
//!
//! JSON-only endpoint for SPA consumption.

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Deserialize, Default)]
pub struct AlertFilter {
    pub r#type: Option<String>,
    pub tenant: Option<String>,
    pub ts_from: Option<i64>,
    pub ts_to: Option<i64>,
    pub format: Option<String>,
}

#[derive(Serialize)]
pub struct AlertDisplayRow {
    pub id: i64,
    pub ts: i64,
    pub event_type: String,
    pub pool_id: String,
    pub tenant_id: String,
    pub model: String,
    pub error_code: String,
    pub msg: String,
}

#[derive(Serialize)]
pub struct AlertsResponse {
    pub events: Vec<AlertDisplayRow>,
    pub event_count: usize,
    pub selected_type: Option<String>,
    pub tenant_filter: String,
    pub ts_from: String,
    pub ts_to: String,
}

pub async fn alerts_handler(
    State(state): State<crate::server::AppState>,
    Query(filter): Query<AlertFilter>,
) -> Result<axum::response::Response, AppError> {
    let tenant_filter = filter.tenant.clone().unwrap_or_default();
    let ts_from = filter.ts_from.map(|v| v.to_string()).unwrap_or_default();
    let ts_to = filter.ts_to.map(|v| v.to_string()).unwrap_or_default();

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
        .map(|r| AlertDisplayRow {
            id: r.id,
            ts: r.ts,
            event_type: r.r#type,
            pool_id: r.pool_id.unwrap_or_default(),
            tenant_id: r.tenant_id.unwrap_or_default(),
            model: r.model.unwrap_or_default(),
            error_code: r.error_code.unwrap_or_default(),
            msg: r.msg.unwrap_or_default(),
        })
        .collect();

    Ok(Json(AlertsResponse {
        events,
        event_count,
        selected_type: filter.r#type,
        tenant_filter,
        ts_from,
        ts_to,
    })
    .into_response())
}

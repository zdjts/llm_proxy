//! Help page — ADR-015 §6 (T69).
//! Content sourced from RUNBOOK.md via build.rs include_str!.

use askama::Template;
use axum::extract::State;
use axum::response::IntoResponse;

use crate::error::AppError;

use super::layout::BaseTemplate;

include!(concat!(env!("OUT_DIR"), "/runbook.rs"));

#[derive(Template)]
#[template(source = "{{ runbook }}", ext = "html")]
struct HelpTemplate {
    runbook: &'static str,
}

/// `GET /admin/help` — help / documentation page.
pub async fn help_handler(
    State(_state): State<crate::server::AppState>,
) -> Result<impl IntoResponse, AppError> {
    let rendered = HelpTemplate { runbook: RUNBOOK }
        .render()
        .map_err(|e| AppError::Internal(format!("template render: {e}")))?;
    let page = BaseTemplate {
        content: rendered,
        is_active_cost: false,
        is_active_requests: false,
        is_active_keys: false,
        is_active_traffic: false,
        is_active_alerts: false,
        is_active_help: true,
    }
    .render()
    .map_err(|e| AppError::Internal(format!("template render: {e}")))?;
    Ok(axum::response::Html(page).into_response())
}

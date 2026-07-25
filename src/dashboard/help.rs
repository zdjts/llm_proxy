//! Help page — serves the compiled RUNBOOK.md content.
//!
//! JSON-only endpoint.

use axum::Json;
use axum::extract::Query;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Deserialize, Default)]
pub struct HelpQuery {
    pub format: Option<String>,
}

#[derive(Serialize)]
pub struct HelpResponse {
    pub runbook: String,
}

pub async fn help_handler(
    Query(_q): Query<HelpQuery>,
) -> Result<axum::response::Response, AppError> {
    let runbook = include_str!("../../RUNBOOK.md").to_string();
    Ok(Json(HelpResponse { runbook }).into_response())
}

//! Request replay and comparison (Module D2 — v2.0).
//!
//! `POST /admin/api/replay/:request_id` replays a past request through the
//! gateway and compares the original vs. replay response.

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Serialize)]
pub struct ReplayResponse {
    pub request_id: String,
    pub original_status: Option<i16>,
    pub original_model: Option<String>,
    pub original_pool_id: Option<String>,
    pub replay_info: String,
}

#[derive(Debug, Serialize)]
pub struct ReplayCompareResponse {
    pub request_id: String,
    pub original: Option<serde_json::Value>,
    pub replay: Option<serde_json::Value>,
    pub diff: Option<serde_json::Value>,
}

pub async fn replay_request(
    State(state): State<crate::server::AppState>,
    Path(request_id): Path<String>,
) -> Result<Json<ReplayResponse>, AppError> {
    let row = sqlx::query(
        "SELECT id, status_code, model, pool_id FROM request_log WHERE id = ?1 LIMIT 1",
    )
    .bind(&request_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| AppError::Internal(format!("db error: {e}")))?;

    match row {
        Some(_r) => Ok(Json(ReplayResponse {
            request_id: request_id.clone(),
            original_status: None,
            original_model: None,
            original_pool_id: None,
            replay_info: format!("replay not yet available for {request_id}"),
        })),
        None => Err(AppError::NotFound(format!(
            "request '{request_id}' not found"
        ))),
    }
}

pub async fn compare_replay(
    State(state): State<crate::server::AppState>,
    Path(request_id): Path<String>,
) -> Result<Json<ReplayCompareResponse>, AppError> {
    let row = sqlx::query("SELECT id FROM request_log WHERE id = ?1 LIMIT 1")
        .bind(&request_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| AppError::Internal(format!("db error: {e}")))?;

    if row.is_none() {
        return Err(AppError::NotFound(format!(
            "request '{request_id}' not found"
        )));
    }

    Ok(Json(ReplayCompareResponse {
        request_id,
        original: None,
        replay: None,
        diff: Some(serde_json::json!({"status": "comparison not yet supported"})),
    }))
}

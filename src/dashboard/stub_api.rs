//! v4.1 Audit-25 Fix: Skeletal API endpoints for pending tracks.
//!
//! Each endpoint returns a JSON response indicating "not yet implemented"
//! with HTTP 501. Frontend pages can use these to display "Coming Soon"
//! states rather than 404 errors. Full implementations tracked in Track M-P.

use crate::error::AppError;
use crate::server::AppState;
use axum::Json;
use axum::extract::State;

fn not_implemented(feature: &str) -> AppError {
    AppError::Internal(format!("{feature}: not yet implemented"))
}

macro_rules! stub_handler {
    ($name:ident, $feature:expr) => {
        pub async fn $name(
            State(_state): State<AppState>,
        ) -> Result<Json<serde_json::Value>, AppError> {
            Err(not_implemented($feature))
        }
    };
}

// ── Model registry (T216) ─────────────────────────────────────────────────
stub_handler!(models_list, "model_registry CRUD (T216)");
stub_handler!(models_create, "model_registry CRUD (T216)");
stub_handler!(models_update, "model_registry CRUD (T216)");
stub_handler!(models_delete, "model_registry CRUD (T216)");

// ── Routing simulator (T219) ──────────────────────────────────────────────
pub async fn routing_simulate(
    State(_state): State<AppState>,
    Json(_body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    Err(not_implemented("routing_simulate (T219)"))
}

// ── Organization CRUD (T222/T223) ─────────────────────────────────────────
stub_handler!(orgs_list, "organization CRUD (T222)");
stub_handler!(orgs_create, "organization CRUD (T222)");
stub_handler!(orgs_update, "organization CRUD (T223)");

// ── Budget spend history (T226) ───────────────────────────────────────────
pub async fn budget_spend_history(
    State(_state): State<AppState>,
    Json(_body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    Err(not_implemented("budget_spend_history (T226)"))
}

// ── Role permissions (T229) ───────────────────────────────────────────────
pub async fn role_permissions(
    State(_state): State<AppState>,
    axum::extract::Path(_role_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    Err(not_implemented("role_permissions (T229)"))
}

// ── Overview aggregation (T234) ───────────────────────────────────────────
stub_handler!(overview_aggregate, "overview_aggregate (T234)");

// ── Request search (T237) ─────────────────────────────────────────────────
pub async fn requests_search(
    State(_state): State<AppState>,
    Json(_body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    Err(not_implemented("requests_search (T237)"))
}

// ── Alert rules CRUD (T243/T244) ──────────────────────────────────────────
stub_handler!(alert_rules_list, "alert_rules CRUD (T243)");
stub_handler!(alert_rules_create, "alert_rules CRUD (T243)");
stub_handler!(alert_rules_update, "alert_rules CRUD (T243)");
stub_handler!(alert_rules_delete, "alert_rules CRUD (T244)");

// ── Alert silences (T247) ─────────────────────────────────────────────────
stub_handler!(alert_silences_list, "alert_silences (T247)");
stub_handler!(alert_silences_create, "alert_silences (T247)");
stub_handler!(alert_silences_delete, "alert_silences (T247)");

// ── Audit log query (T248/T249) ───────────────────────────────────────────
pub async fn audit_log_query(
    State(_state): State<AppState>,
    axum::extract::Query(_params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    Err(not_implemented("audit_log_query (T248)"))
}

// ── System settings (T250/T251) ───────────────────────────────────────────
stub_handler!(settings_get, "settings_get (T250)");
stub_handler!(settings_update, "settings_update (T251)");

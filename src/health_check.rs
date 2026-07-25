//! Health check endpoints — Kubernetes-compatible liveness/readiness probes.
//!
//! GET /health/live   — Always returns 200 if the server is running.
//! GET /health/ready  — Returns 200 only if all critical dependencies are healthy.
//! GET /health        — Returns a JSON summary of all health checks.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Serialize;

use crate::error::AppError;

#[derive(Serialize)]
pub struct HealthReport {
    pub status: String,
    pub checks: Vec<HealthCheck>,
    pub uptime_secs: u64,
}

#[derive(Serialize)]
pub struct HealthCheck {
    pub name: String,
    pub status: String,
    pub message: String,
}

pub async fn health_live() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

pub async fn health_ready(
    State(state): State<crate::server::AppState>,
) -> Result<impl IntoResponse, AppError> {
    // Check database
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();

    // Check that at least one pool has keys
    let has_pools = !state.router.current().model_list().is_empty();

    if db_ok && has_pools {
        Ok((StatusCode::OK, "READY"))
    } else {
        Ok((StatusCode::SERVICE_UNAVAILABLE, "NOT READY"))
    }
}

pub async fn health_full(
    State(state): State<crate::server::AppState>,
) -> Result<Json<HealthReport>, AppError> {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();
    let models = state.router.current().model_list();
    let has_pools = !models.is_empty();

    let mut checks = Vec::new();
    checks.push(HealthCheck {
        name: "database".into(),
        status: if db_ok { "ok".into() } else { "fail".into() },
        message: if db_ok {
            "SQLite connected".into()
        } else {
            "SQLite unreachable".into()
        },
    });
    checks.push(HealthCheck {
        name: "models".into(),
        status: if has_pools {
            "ok".into()
        } else {
            "fail".into()
        },
        message: format!("{} models configured", models.len()),
    });
    checks.push(HealthCheck {
        name: "server".into(),
        status: "ok".into(),
        message: "axum http server running".into(),
    });

    let all_ok = checks.iter().all(|c| c.status == "ok");

    Ok(Json(HealthReport {
        status: if all_ok {
            "healthy".into()
        } else {
            "degraded".into()
        },
        checks,
        uptime_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    }))
}

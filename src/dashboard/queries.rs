//! Shared dashboard SQL helpers (read-only aggregations).
//!
//! Accounting cost still uses ConfigStore pricing, not model_metadata (ADR-017).

use sqlx::SqlitePool;

use crate::error::AppError;

pub async fn query_tenant_list(pool: &SqlitePool) -> Result<Vec<String>, AppError> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT tenant_id FROM request_log ORDER BY tenant_id")
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Internal(format!("tenant list query: {e}")))?;
    Ok(rows.into_iter().map(|(t,)| t).collect())
}

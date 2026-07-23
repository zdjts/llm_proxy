//! SQLite persistence for AlertEvent — ADR-013 §2.

use sqlx::SqlitePool;

use crate::error::AppError;

use super::AlertEvent;

/// Row-mapped alert_event table row.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SchemaAlertEvent {
    pub id: i64,
    pub ts: i64,
    pub r#type: String,
    pub pool_id: Option<String>,
    pub tenant_id: Option<String>,
    pub key_hash: Option<String>,
    pub model: Option<String>,
    pub error_code: Option<String>,
    pub status: Option<i64>,
    pub msg: Option<String>,
    pub payload: Option<String>,
}

/// Insert an AlertEvent into the alert_event table.
pub async fn insert_alert_event(pool: &SqlitePool, event: &AlertEvent) -> Result<(), AppError> {
    let payload = serde_json::to_string(event).unwrap_or_default();
    let ts_secs = event.ts() / 1000;

    sqlx::query(
        "INSERT INTO alert_event (ts, type, pool_id, tenant_id, key_hash, model, error_code, status, msg, payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(ts_secs)
    .bind(event.event_type())
    .bind(event.pool_id_str())
    .bind(event.tenant_id_str())
    .bind(event.key_hash_str())
    .bind(event.model_str())
    .bind(event.error_code_str())
    .bind(event.status_code())
    .bind(event.msg_str())
    .bind(payload)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(format!("alert insert: {e}")))?;

    Ok(())
}

/// Query recent events with optional filters.
pub async fn query_alert_events(
    pool: &SqlitePool,
    limit: i64,
    type_filter: &Option<String>,
    tenant_filter: &Option<String>,
    ts_from: &Option<i64>,
    ts_to: &Option<i64>,
) -> Result<Vec<SchemaAlertEvent>, AppError> {
    let rows: Vec<SchemaAlertEvent> = sqlx::query_as(
        "SELECT id, ts, type, pool_id, tenant_id, key_hash, model, error_code, status, msg, payload \
         FROM alert_event \
         WHERE (?1 IS NULL OR type = ?1) \
           AND (?2 IS NULL OR tenant_id = ?2) \
           AND (?3 IS NULL OR ts >= ?3) \
           AND (?4 IS NULL OR ts <= ?4) \
         ORDER BY ts DESC \
         LIMIT ?5",
    )
    .bind(type_filter)
    .bind(tenant_filter)
    .bind(ts_from)
    .bind(ts_to)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(format!("alert query: {e}")))?;

    Ok(rows)
}

// ── AlertEvent helper accessors ────────────────────────────────────────

impl AlertEvent {
    pub fn ts(&self) -> i64 {
        match self {
            Self::UpstreamError { ts, .. } => *ts,
            Self::LatencySpike { ts, .. } => *ts,
            Self::RateLimited { ts, .. } => *ts,
            Self::PoolExhausted { ts, .. } => *ts,
        }
    }

    pub fn event_type(&self) -> &str {
        match self {
            Self::UpstreamError { .. } => "UpstreamError",
            Self::LatencySpike { .. } => "LatencySpike",
            Self::RateLimited { .. } => "RateLimited",
            Self::PoolExhausted { .. } => "PoolExhausted",
        }
    }

    pub fn pool_id_str(&self) -> Option<&str> {
        match self {
            Self::UpstreamError { pool_id, .. } => Some(pool_id),
            Self::PoolExhausted { pool_id, .. } => Some(pool_id),
            _ => None,
        }
    }

    pub fn tenant_id_str(&self) -> Option<&str> {
        match self {
            Self::RateLimited { tenant_id, .. } => Some(tenant_id),
            _ => None,
        }
    }

    pub fn key_hash_str(&self) -> Option<&str> {
        match self {
            Self::UpstreamError { key_hash, .. } => Some(key_hash),
            _ => None,
        }
    }

    pub fn model_str(&self) -> Option<&str> {
        match self {
            Self::LatencySpike { model, .. } => Some(model),
            _ => None,
        }
    }

    pub fn error_code_str(&self) -> Option<&str> {
        match self {
            Self::UpstreamError { error_code, .. } => Some(error_code),
            _ => None,
        }
    }

    fn status_code(&self) -> Option<i64> {
        match self {
            Self::UpstreamError { status, .. } => status.map(|s| s as i64),
            _ => None,
        }
    }

    pub fn msg_str(&self) -> Option<&str> {
        match self {
            Self::UpstreamError { msg, .. } => Some(msg),
            Self::LatencySpike { .. } => Some("latency threshold exceeded"),
            Self::RateLimited { .. } => Some("rate limited"),
            Self::PoolExhausted { .. } => Some("all keys exhausted"),
        }
    }
}

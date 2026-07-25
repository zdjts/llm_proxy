//! Audit trail recording (Track C — T137).
//!
//! Centralized helper for recording all admin management operations into the
//! `audit_trail` table. Every write operation in T90 (key CRUD), T96 (routing
//! edit), T101 (pipeline edit), T105 (quota config), T111 (alert rule config),
//! T114 (team management), and T116 (SSO config) MUST call this module rather
//! than implementing ad-hoc logging.
//!
//! # Usage
//!
//! ```ignore
//! use crate::audit_trail::{record_audit, AuditEvent};
//! record_audit(&pool, AuditEvent {
//!     event_type: "key.rotate".into(),
//!     actor_id: Some("user-123".into()),
//!     actor_ip: Some("10.0.0.1".into()),
//!     target_type: "client_key".into(),
//!     target_id: "key-hash-abc123".into(),
//!     before_json: Some(serde_json::to_value(&old_record)?),
//!     after_json: Some(serde_json::to_value(&new_record)?),
//!     metadata: None,
//! }).await?;
//! ```

use sqlx::SqlitePool;

use crate::error::AppError;

/// A single audit trail entry.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    /// e.g. "key.create", "key.rotate", "routing.update", "pipeline.save", "quota.set"
    pub event_type: String,
    /// `user_account.id` who performed the action (None for system actions)
    pub actor_id: Option<String>,
    /// IP address of the actor
    pub actor_ip: Option<String>,
    /// What kind of resource: "client_key", "provider", "route", "pipeline", "quota", "alert_rule", "user", "team"
    pub target_type: String,
    /// Identifier of the affected resource
    pub target_id: String,
    /// JSON snapshot before change (None = creation)
    pub before_json: Option<serde_json::Value>,
    /// JSON snapshot after change (None = deletion)
    pub after_json: Option<serde_json::Value>,
    /// Extra metadata (e.g. user-agent, reason for change)
    pub metadata: Option<serde_json::Value>,
}

/// Record an audit trail entry. Returns immediately on DB error (logs warning, does not fail the caller).
pub async fn record_audit(pool: &SqlitePool, event: AuditEvent) -> Result<(), AppError> {
    let before = event
        .before_json
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_default();
    let after = event
        .after_json
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_default();
    let meta = event
        .metadata
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_default();

    sqlx::query(
        "INSERT INTO audit_trail (event_type, actor_id, actor_ip, target_type, target_id, before_json, after_json, metadata) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&event.event_type)
    .bind(&event.actor_id)
    .bind(&event.actor_ip)
    .bind(&event.target_type)
    .bind(&event.target_id)
    .bind(&before)
    .bind(&after)
    .bind(&meta)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(format!("Failed to record audit trail: {e}")))?;

    Ok(())
}

/// Versioned wrapper that records both before and after states for an update operation.
#[allow(clippy::too_many_arguments)]
pub async fn record_update<T: serde::Serialize>(
    pool: &SqlitePool,
    event_type: &str,
    actor_id: Option<&str>,
    actor_ip: Option<&str>,
    target_type: &str,
    target_id: &str,
    before: &T,
    after: &T,
) -> Result<(), AppError> {
    record_audit(
        pool,
        AuditEvent {
            event_type: event_type.to_string(),
            actor_id: actor_id.map(String::from),
            actor_ip: actor_ip.map(String::from),
            target_type: target_type.to_string(),
            target_id: target_id.to_string(),
            before_json: Some(serde_json::to_value(before).unwrap_or_default()),
            after_json: Some(serde_json::to_value(after).unwrap_or_default()),
            metadata: None,
        },
    )
    .await
}

/// Query audit trail entries with optional filters. All user-supplied values
/// use parameterized bindings — no string concatenation into SQL (Fix 8).
#[allow(clippy::too_many_arguments)]
pub async fn query_audit_trail(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
    event_type: Option<&str>,
    actor_id: Option<&str>,
    target_type: Option<&str>,
    target_id: Option<&str>,
) -> Result<Vec<AuditTrailRow>, AppError> {
    let mut clauses: Vec<&str> = Vec::new();
    // Collect bindable values in order
    let mut bind_values: Vec<String> = Vec::new();

    if let Some(v) = event_type {
        clauses.push("event_type = ?");
        bind_values.push(v.to_string());
    }
    if let Some(v) = actor_id {
        // actor_id is the Nth bind param — we'll number them later
        clauses.push("actor_id = ?");
        bind_values.push(v.to_string());
    }
    if let Some(v) = target_type {
        clauses.push("target_type = ?");
        bind_values.push(v.to_string());
    }
    if let Some(v) = target_id {
        clauses.push("target_id = ?");
        bind_values.push(v.to_string());
    }

    // Build the WHERE clause with numbered parameters
    let where_clause = if clauses.is_empty() {
        "1=1".to_string()
    } else {
        clauses
            .iter()
            .enumerate()
            .map(|(i, clause)| {
                // Replace the `?` with `?N` (1-indexed for SQLite)
                clause.replace('?', &format!("?{}", i + 1))
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    };

    // LIMIT and OFFSET are also parameterized (they come after the filter binds)
    let limit_idx = bind_values.len() + 1;
    let offset_idx = bind_values.len() + 2;

    let sql = format!(
        "SELECT id, event_type, actor_id, actor_ip, target_type, target_id, \
         before_json, after_json, metadata, created_at \
         FROM audit_trail WHERE {where_clause} \
         ORDER BY created_at DESC LIMIT ?{limit_idx} OFFSET ?{offset_idx}"
    );

    let mut query = sqlx::query_as::<_, AuditTrailRow>(&sql);
    for val in &bind_values {
        query = query.bind(val);
    }
    query = query.bind(limit).bind(offset);

    query
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to query audit trail: {e}")))
}

#[derive(Debug, sqlx::FromRow)]
pub struct AuditTrailRow {
    #[allow(dead_code)]
    pub id: i64,
    pub event_type: String,
    pub actor_id: Option<String>,
    pub actor_ip: Option<String>,
    pub target_type: String,
    pub target_id: String,
    pub before_json: String,
    pub after_json: String,
    pub metadata: String,
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup() -> (SqlitePool, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test_audit.db");
        let pool = crate::db::connect(path.to_str().unwrap()).await.unwrap();
        (pool, dir)
    }

    #[tokio::test]
    async fn it_records_and_queries_audit_event() {
        let (pool, _dir) = setup().await;

        record_audit(
            &pool,
            AuditEvent {
                event_type: "key.create".into(),
                actor_id: Some("user-1".into()),
                actor_ip: Some("127.0.0.1".into()),
                target_type: "client_key".into(),
                target_id: "hash-abc".into(),
                before_json: None,
                after_json: Some(serde_json::json!({"key_hash": "hash-abc", "tenant": "t1"})),
                metadata: None,
            },
        )
        .await
        .unwrap();

        let rows = query_audit_trail(&pool, 10, 0, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_type, "key.create");
        assert_eq!(rows[0].actor_id.as_deref(), Some("user-1"));
    }

    #[tokio::test]
    async fn it_filters_by_event_type() {
        let (pool, _dir) = setup().await;

        for (i, et) in ["key.create", "key.rotate", "routing.update"]
            .iter()
            .enumerate()
        {
            record_audit(
                &pool,
                AuditEvent {
                    event_type: et.to_string(),
                    actor_id: Some(format!("user-{i}")),
                    actor_ip: None,
                    target_type: "test".into(),
                    target_id: format!("target-{i}"),
                    before_json: None,
                    after_json: None,
                    metadata: None,
                },
            )
            .await
            .unwrap();
        }

        let rows = query_audit_trail(&pool, 10, 0, Some("key.create"), None, None, None)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_type, "key.create");
    }

    #[tokio::test]
    async fn it_records_update_with_before_after() {
        let (pool, _dir) = setup().await;

        let before = serde_json::json!({"weight": 1});
        let after = serde_json::json!({"weight": 5});

        record_update(
            &pool,
            "routing.update",
            Some("user-1"),
            None,
            "route",
            "model-gpt4o",
            &before,
            &after,
        )
        .await
        .unwrap();

        let rows = query_audit_trail(&pool, 10, 0, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].before_json.contains("\"weight\":1"));
        assert!(rows[0].after_json.contains("\"weight\":5"));
    }

    #[tokio::test]
    async fn it_respects_limit_and_offset() {
        let (pool, _dir) = setup().await;

        for i in 0..5 {
            record_audit(
                &pool,
                AuditEvent {
                    event_type: "test".into(),
                    actor_id: None,
                    actor_ip: None,
                    target_type: "t".into(),
                    target_id: format!("{i}"),
                    before_json: None,
                    after_json: None,
                    metadata: None,
                },
            )
            .await
            .unwrap();
        }

        let rows = query_audit_trail(&pool, 2, 1, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
    }
}

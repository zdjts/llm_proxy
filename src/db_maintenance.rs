//! Database maintenance background tasks (T139 enhanced for v3.0).
//!
//! - Reads per-table retention policies from `data_retention_policy`
//! - Hourly purge of expired rows across all managed tables
//! - Periodic VACUUM / PRAGMA optimize
//! - FTS5 index incremental rebuild for request_log full-text search

use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::watch;

pub struct DbMaintenanceConfig {
    pub vacuum_interval_hours: u64,
    pub purge_interval_hours: u64,
    pub fts_rebuild_interval_hours: u64,
}

impl Default for DbMaintenanceConfig {
    fn default() -> Self {
        Self {
            vacuum_interval_hours: 24,
            purge_interval_hours: 1,
            fts_rebuild_interval_hours: 6,
        }
    }
}

pub fn spawn_db_maintenance(
    pool: SqlitePool,
    config: DbMaintenanceConfig,
    mut shutdown_rx: watch::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let vacuum_interval = Duration::from_secs(config.vacuum_interval_hours * 3600);
        let purge_interval = Duration::from_secs(config.purge_interval_hours * 3600);
        let fts_interval = Duration::from_secs(config.fts_rebuild_interval_hours * 3600);

        let mut vacuum_tick = tokio::time::interval(vacuum_interval);
        let mut purge_tick = tokio::time::interval(purge_interval);
        let mut fts_tick = tokio::time::interval(fts_interval);

        // Suppress first immediate tick
        vacuum_tick.tick().await;
        purge_tick.tick().await;
        fts_tick.tick().await;

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    tracing::info!("db maintenance shutting down");
                    break;
                }
                _ = purge_tick.tick() => {
                    purge_expired(&pool).await;
                }
                _ = vacuum_tick.tick() => {
                    vacuum(&pool).await;
                }
                _ = fts_tick.tick() => {
                    rebuild_fts(&pool).await;
                }
            }
        }
    })
}

async fn purge_expired(pool: &SqlitePool) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // Read retention policies from DB
    let policies: Vec<(String, i64)> = match sqlx::query_as::<_, PolicyRow>(
        "SELECT table_name, retention_days FROM data_retention_policy",
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows
            .into_iter()
            .map(|r| (r.table_name, r.retention_days))
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to read retention policies, skipping purge");
            return;
        }
    };

    for (table_name, retention_days) in &policies {
        // Map tables to their timestamp column
        let (ts_column, is_ms) = match table_name.as_str() {
            "request_log" => ("ts", true),
            "audit_hourly" => ("hour_ts", true),
            "audit_trail" => ("created_at", true),
            "ui_event_log" => ("created_at", true),
            "alert_event" => ("created_at", true),
            _ => {
                tracing::debug!(table = %table_name, "Unknown table for retention, skipping");
                continue;
            }
        };

        let cutoff = if is_ms {
            now_ms - retention_days * 86_400_000
        } else {
            now_ms / 1000 - retention_days * 86_400
        };

        let sql = format!("DELETE FROM {table_name} WHERE {ts_column} < ?1");
        match sqlx::query(&sql).bind(cutoff).execute(pool).await {
            Ok(result) => {
                let deleted = result.rows_affected();
                if deleted > 0 {
                    tracing::info!(
                        table = %table_name,
                        deleted,
                        retention_days,
                        "Purged expired rows"
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    table = %table_name,
                    error = %e,
                    "Failed to purge expired rows"
                );
            }
        }
    }
}

async fn vacuum(pool: &SqlitePool) {
    match sqlx::query("PRAGMA optimize").execute(pool).await {
        Ok(_) => tracing::debug!("Database optimized"),
        Err(e) => tracing::warn!(error = %e, "Database optimize failed"),
    }
}

/// Incrementally rebuild the FTS5 index for request_log messages.
///
/// # Current limitations (Fix 9)
///
/// The `request_log` table stores the model name and token counts, but NOT the
/// actual message content (prompt text). Therefore the FTS5 index currently
/// indexes only the model name, which is sufficient for model-based filtering
/// but NOT for full-text search over conversation content.
///
/// To enable true full-text search over messages (T93), one of the following
/// must happen in a future PR:
/// - Add a `messages_text` TEXT column to `request_log` populated at insert time
/// - Store messages in a separate `request_messages` table linked by request_id
/// - Use an external log aggregation system (Loki/ELK) for content search
///
/// Until then, this FTS5 index provides model-name search; the `requests`
/// filter builder UI should make this limitation clear.
async fn rebuild_fts(pool: &SqlitePool) {
    // Watermark tracks the last `ts` value that was fully processed.
    // The column is named `last_ts` in the DB but the Rust struct still
    // calls it `last_rowid` — this is historical; the value IS a timestamp.
    let watermark_ts: i64 = match sqlx::query_as::<_, WatermarkRow>(
        "SELECT last_rowid FROM request_log_fts_watermark WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
    {
        Ok(Some(row)) => row.last_rowid,
        _ => 0,
    };

    // Find request_log entries newer than watermark
    let rows: Vec<MessageTextRow> = match sqlx::query_as::<_, MessageTextRow>(
        "SELECT id, ts, model FROM request_log WHERE ts > ?1 ORDER BY ts ASC LIMIT 1000",
    )
    .bind(watermark_ts)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "FTS rebuild: failed to read request_log");
            return;
        }
    };

    if rows.is_empty() {
        return;
    }

    // ── Fix 10: track the max ts of actually-processed rows for the watermark ──
    let max_processed_ts = rows.last().map(|r| r.ts).unwrap_or(watermark_ts);
    let mut indexed = 0usize;

    for row in &rows {
        let text = format!("model:{}", row.model.as_deref().unwrap_or("unknown"));

        if let Err(e) = sqlx::query(
            "INSERT OR REPLACE INTO request_log_fts (request_id, messages_text) VALUES (?1, ?2)",
        )
        .bind(&row.id)
        .bind(&text)
        .execute(pool)
        .await
        {
            tracing::warn!(request_id = %row.id, error = %e, "FTS index insert failed");
        } else {
            indexed += 1;
        }
    }

    // ── Fix 10: watermark set to max ts of actually-processed batch ──
    // If the batch was full (1000 rows), the next cycle will pick up from here.
    // This avoids the "permanently skip rows" bug where the watermark jumped
    // to wall-clock time, skipping rows with ts between batch-end and wall-clock.
    if let Err(e) = sqlx::query("UPDATE request_log_fts_watermark SET last_rowid = ?1 WHERE id = 1")
        .bind(max_processed_ts)
        .execute(pool)
        .await
    {
        tracing::warn!(error = %e, "FTS watermark update failed");
    }

    if indexed > 0 {
        tracing::info!(
            indexed,
            watermark = max_processed_ts,
            "FTS5 incremental index updated"
        );
    }
}

#[derive(sqlx::FromRow)]
struct PolicyRow {
    table_name: String,
    retention_days: i64,
}

#[derive(sqlx::FromRow)]
struct WatermarkRow {
    last_rowid: i64,
}

#[derive(sqlx::FromRow)]
struct MessageTextRow {
    id: String,
    ts: i64,
    model: Option<String>,
}

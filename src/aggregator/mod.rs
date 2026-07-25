//! Hourly audit-data aggregation background task.
//!
//! Runs once per wall-clock hour (on the hour), reads the previous hour's
//! `request_log` rows, and upserts aggregated results into `audit_hourly`.
//! UPSERT semantics make the task restart-idempotent.

use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use crate::error::AppError;

/// Spawn the hourly aggregation task onto a tokio runtime.
pub fn spawn_aggregator(
    pool: SqlitePool,
    mut shutdown_rx: watch::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    tracing::info!("aggregator shutting down");
                    break;
                }
                _ = interval.tick() => {}
            }

            if let Err(e) = aggregate_last_hour(&pool).await {
                tracing::error!(error = %e, "aggregator failed, will retry next hour");
            }
        }
    })
}

/// Aggregate the most recently completed wall-clock hour into `audit_hourly`.
async fn aggregate_last_hour(pool: &SqlitePool) -> Result<(), AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let hour_start = (now / 3600 - 1) * 3600;
    let hour_end = hour_start + 3600;

    sqlx::query(
        "INSERT INTO audit_hourly \
         (hour, model, pool_id, key_hash, cache_source, error_code, finish_reason, upstream_model, tenant_id, \
          request_count, success_count, retry_total, \
          prompt_tokens, completion_tokens, total_tokens, \
          cached_tokens, reasoning_tokens, audio_tokens, cost_usd, \
          latency_ms_sum, ttft_ms_sum, stream_count) \
         SELECT ?1, model, pool_id, key_hash, COALESCE(cache_source, ''), COALESCE(error_code, ''), COALESCE(finish_reason, ''), COALESCE(upstream_model, ''), COALESCE(tenant_id, 'default'), \
                COUNT(*) AS request_count, \
                SUM(CASE WHEN status_code >= 200 AND status_code < 300 THEN 1 ELSE 0 END) AS success_count, \
                SUM(COALESCE(retry_count, 0)) AS retry_total, \
                SUM(COALESCE(prompt_tokens, 0)) AS prompt_tokens, \
                SUM(COALESCE(completion_tokens, 0)) AS completion_tokens, \
                SUM(COALESCE(total_tokens, 0)) AS total_tokens, \
                SUM(COALESCE(cached_tokens, 0)) AS cached_tokens, \
                SUM(COALESCE(reasoning_tokens, 0)) AS reasoning_tokens, \
                SUM(COALESCE(audio_tokens, 0)) AS audio_tokens, \
                SUM(COALESCE(cost_usd, 0)) AS cost_usd, \
                SUM(COALESCE(latency_ms, 0)) AS latency_ms_sum, \
                SUM(COALESCE(ttft_ms, 0)) AS ttft_ms_sum, \
                SUM(CASE WHEN is_stream != 0 THEN 1 ELSE 0 END) AS stream_count \
         FROM request_log \
         WHERE ts >= ?2 AND ts < ?3 \
         GROUP BY model, pool_id, key_hash, COALESCE(cache_source, ''), COALESCE(error_code, ''), COALESCE(finish_reason, ''), COALESCE(upstream_model, ''), COALESCE(tenant_id, 'default') \
         ON CONFLICT(hour, model, pool_id, key_hash, cache_source, error_code, finish_reason, upstream_model, tenant_id) \
         DO UPDATE SET \
           request_count       = excluded.request_count, \
           success_count       = excluded.success_count, \
           retry_total         = excluded.retry_total, \
           prompt_tokens       = excluded.prompt_tokens, \
           completion_tokens   = excluded.completion_tokens, \
           total_tokens        = excluded.total_tokens, \
           cached_tokens       = excluded.cached_tokens, \
           reasoning_tokens    = excluded.reasoning_tokens, \
           audio_tokens        = excluded.audio_tokens, \
           cost_usd            = excluded.cost_usd, \
           latency_ms_sum      = excluded.latency_ms_sum, \
           ttft_ms_sum         = excluded.ttft_ms_sum, \
           stream_count        = excluded.stream_count",
    )
    .bind(hour_start)
    .bind(hour_start * 1000)
    .bind(hour_end * 1000)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(format!("aggregation failed: {e}")))?;

    tracing::info!(hour = hour_start, "aggregated audit data for hour");
    Ok(())
}

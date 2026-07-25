//! Key health / failover screen — ADR-007 §10 (T17).
//!
//! JSON-only endpoint. Reads in-memory state from `Router::pool_snapshot()`.

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;

#[derive(Deserialize, Default)]
pub struct KeysQuery {
    pub format: Option<String>,
}

#[derive(Serialize)]
pub struct KeysResponse {
    pub pools: Vec<PoolView>,
}

#[derive(Serialize)]
pub struct PoolView {
    pub pool_id: String,
    pub keys: Vec<KeyView>,
}

#[derive(Serialize)]
pub struct KeyView {
    pub key_hash: String,
    pub weight: u32,
    pub healthy: bool,
    #[serde(rename = "sparkline")]
    pub sparkline: Vec<u32>,
    pub success_rate: String,
}

impl KeyView {
    pub fn from_snapshot(ks: crate::router::KeySnapshot, spark: Vec<u32>, rate: String) -> Self {
        Self {
            key_hash: ks.key_hash,
            weight: ks.weight,
            healthy: ks.healthy,
            sparkline: spark,
            success_rate: rate,
        }
    }
}

pub async fn key_health_handler(
    State(state): State<crate::server::AppState>,
    _q: Query<KeysQuery>,
) -> Result<axum::response::Response, AppError> {
    let snaps = state.router.current().pool_snapshot();
    let mut pools = Vec::new();

    for snap in snaps {
        let mut keys = Vec::new();
        for ks in snap.keys {
            let (spark, rate) = key_success_rate(&state.db, &ks.key_hash).await;
            keys.push(KeyView {
                key_hash: ks.key_hash,
                weight: ks.weight,
                healthy: ks.healthy,
                sparkline: spark,
                success_rate: rate,
            });
        }
        pools.push(PoolView {
            pool_id: snap.pool_id,
            keys,
        });
    }

    Ok(Json(KeysResponse { pools }).into_response())
}

pub async fn key_success_rate_for_hash(pool: &SqlitePool, key_hash: &str) -> (Vec<u32>, String) {
    key_success_rate(pool, key_hash).await
}

async fn key_success_rate(pool: &SqlitePool, key_hash: &str) -> (Vec<u32>, String) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let window_start = (now / 3600 - 7 * 24) * 3600;

    let rows: Result<Vec<DayBucket>, _> = sqlx::query_as(
        "SELECT (hour / 86400) * 86400 AS day, \
                SUM(success_count) AS success, \
                SUM(request_count) AS total \
         FROM audit_hourly \
         WHERE key_hash = ?1 AND hour >= ?2 \
         GROUP BY day \
         ORDER BY day",
    )
    .bind(key_hash)
    .bind(window_start)
    .fetch_all(pool)
    .await;

    let rows = match rows {
        Ok(r) => r,
        Err(_) => return (vec![], "—".into()),
    };

    let max_total = rows.iter().map(|r| r.total).max().unwrap_or(1).max(1);
    let sparkline: Vec<u32> = rows
        .iter()
        .map(|r| ((r.success as f64 / max_total as f64) * 20.0) as u32)
        .collect();

    let total_success: i64 = rows.iter().map(|r| r.success).sum();
    let total_all: i64 = rows.iter().map(|r| r.total).sum();
    let rate = if total_all > 0 {
        format!("{:.1}%", total_success as f64 / total_all as f64 * 100.0)
    } else {
        "—".into()
    };

    (sparkline, rate)
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct DayBucket {
    day: i64,
    success: i64,
    total: i64,
}

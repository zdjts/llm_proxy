//! Admin REST API — endpoints for external management tools (v2.0).
//!
//! Extended with client-key CRUD, key rotation, and quota management.

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use std::sync::atomic::Ordering;

use crate::audit_trail;
use crate::config_store::ConfigStore;
use crate::error::AppError;

// ── Reusable response types ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatusResponse {
    pub uptime_secs: u64,
    pub requests_total: u64,
    pub requests_failed: u64,
    pub stream_requests: u64,
    pub cache_hits: u64,
    pub active_connections: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub retries: u64,
    pub key_demotions: u64,
    pub upstream_5xx: u64,
    pub upstream_4xx: u64,
    pub pools: Vec<PoolStatus>,
    pub alert_count: usize,
}

#[derive(Serialize)]
pub struct PoolStatus {
    pub pool_id: String,
    pub total_keys: usize,
    pub healthy_keys: usize,
    pub bad_keys: usize,
}

#[derive(Serialize)]
pub struct KeysResponse {
    pub pools: Vec<super::keys::PoolView>,
}

#[derive(Serialize)]
pub struct ReloadResponse {
    pub ok: bool,
    pub message: String,
}

// ── Standard status / keys / reload ───────────────────────────────────────

pub async fn admin_api_status(
    State(state): State<crate::server::AppState>,
) -> Result<Json<StatusResponse>, AppError> {
    let snaps = state.router.current().pool_snapshot();
    let pools: Vec<PoolStatus> = snaps
        .into_iter()
        .map(|s| {
            let total = s.keys.len();
            let healthy = s.keys.iter().filter(|k| k.healthy).count();
            PoolStatus {
                pool_id: s.pool_id,
                total_keys: total,
                healthy_keys: healthy,
                bad_keys: total - healthy,
            }
        })
        .collect();

    let alert_count = state.alert_snapshot.lock().unwrap().len();

    Ok(Json(StatusResponse {
        uptime_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        requests_total: state.metrics.total_requests.load(Ordering::Relaxed),
        requests_failed: state.metrics.failed_requests.load(Ordering::Relaxed),
        stream_requests: state.metrics.stream_requests.load(Ordering::Relaxed),
        cache_hits: state.metrics.cache_hits.load(Ordering::Relaxed),
        active_connections: state.metrics.active_connections.load(Ordering::Relaxed),
        prompt_tokens: state.metrics.total_prompt_tokens.load(Ordering::Relaxed),
        completion_tokens: state
            .metrics
            .total_completion_tokens
            .load(Ordering::Relaxed),
        retries: state.metrics.retry_count.load(Ordering::Relaxed),
        key_demotions: state.metrics.key_demotions.load(Ordering::Relaxed),
        upstream_5xx: state.metrics.upstream_5xx.load(Ordering::Relaxed),
        upstream_4xx: state.metrics.upstream_4xx.load(Ordering::Relaxed),
        pools,
        alert_count,
    }))
}

pub async fn admin_api_keys(
    State(state): State<crate::server::AppState>,
) -> Result<Json<KeysResponse>, AppError> {
    let snaps = state.router.current().pool_snapshot();
    let mut pools = Vec::new();

    for snap in snaps {
        let mut keys = Vec::new();
        for ks in snap.keys {
            let (spark, rate) =
                super::keys::key_success_rate_for_hash(&state.db, &ks.key_hash).await;
            keys.push(super::keys::KeyView::from_snapshot(ks, spark, rate));
        }
        pools.push(super::keys::PoolView {
            pool_id: snap.pool_id,
            keys,
        });
    }

    Ok(Json(KeysResponse { pools }))
}

pub async fn admin_api_reload() -> Result<Json<ReloadResponse>, AppError> {
    Ok(Json(ReloadResponse {
        ok: true,
        message: "Send SIGHUP to the gateway process to reload config, or use /admin/api/reload-notify for broadcast".into(),
    }))
}

// ── Client key CRUD (Module A1 — v2.0) ────────────────────────────────────

#[derive(Serialize)]
pub struct ClientKeyListResponse {
    pub keys: Vec<crate::auth_store::ClientKeyRecord>,
    pub total: usize,
}

pub async fn admin_api_list_client_keys(
    State(state): State<crate::server::AppState>,
) -> Result<Json<ClientKeyListResponse>, AppError> {
    match &state.auth_store {
        Some(store) => {
            let keys = store.list();
            let total = keys.len();
            Ok(Json(ClientKeyListResponse { keys, total }))
        }
        None => Err(AppError::Auth("auth store not initialized".into())),
    }
}

pub async fn admin_api_add_client_key(
    State(state): State<crate::server::AppState>,
    Json(req): Json<crate::auth_store::CreateKeyRequest>,
) -> Result<Json<crate::auth_store::ClientKeyRecord>, AppError> {
    match &state.auth_store {
        Some(store) => {
            let record = store.add(req)?;
            // ── v3.0 audit trail (Fix 7) ──
            let _ = audit_trail::record_audit(
                &state.db,
                audit_trail::AuditEvent {
                    event_type: "key.create".into(),
                    actor_id: None, // populated when RBAC is fully integrated
                    actor_ip: None,
                    target_type: "client_key".into(),
                    target_id: record.key_hash.clone(),
                    before_json: None,
                    after_json: Some(serde_json::to_value(&record).unwrap_or_default()),
                    metadata: None,
                },
            )
            .await;
            Ok(Json(record))
        }
        None => Err(AppError::Auth("auth store not initialized".into())),
    }
}

pub async fn admin_api_update_client_key(
    State(state): State<crate::server::AppState>,
    Path(key_hash): Path<String>,
    Json(req): Json<crate::auth_store::UpdateKeyRequest>,
) -> Result<Json<crate::auth_store::ClientKeyRecord>, AppError> {
    match &state.auth_store {
        Some(store) => {
            let record = store.update(&key_hash, req)?;
            Ok(Json(record))
        }
        None => Err(AppError::Auth("auth store not initialized".into())),
    }
}

pub async fn admin_api_delete_client_key(
    State(state): State<crate::server::AppState>,
    Path(key_hash): Path<String>,
) -> Result<Json<crate::auth_store::ClientKeyRecord>, AppError> {
    match &state.auth_store {
        Some(store) => {
            // Snapshot before deletion for audit trail
            let before = store.get_by_hash(&key_hash);
            let record = store.remove(&key_hash)?;
            // ── v3.0 audit trail (Fix 7) ──
            let _ = audit_trail::record_audit(
                &state.db,
                audit_trail::AuditEvent {
                    event_type: "key.delete".into(),
                    actor_id: None,
                    actor_ip: None,
                    target_type: "client_key".into(),
                    target_id: key_hash.clone(),
                    before_json: before.as_ref().and_then(|r| serde_json::to_value(r).ok()),
                    after_json: None,
                    metadata: None,
                },
            )
            .await;
            Ok(Json(record))
        }
        None => Err(AppError::Auth("auth store not initialized".into())),
    }
}

pub async fn admin_api_rotate_client_key(
    State(state): State<crate::server::AppState>,
    Path(key_hash): Path<String>,
    Json(req): Json<crate::auth_store::RotateKeyRequest>,
) -> Result<Json<crate::auth_store::ClientKeyRecord>, AppError> {
    match &state.auth_store {
        Some(store) => {
            let record = store.rotate(&key_hash, req)?;
            // ── v3.0 audit trail (Fix 7) ──
            let _ = audit_trail::record_audit(
                &state.db,
                audit_trail::AuditEvent {
                    event_type: "key.rotate".into(),
                    actor_id: None,
                    actor_ip: None,
                    target_type: "client_key".into(),
                    target_id: record.key_hash.clone(),
                    before_json: Some(serde_json::json!({"old_key_hash": &key_hash})),
                    after_json: Some(serde_json::json!({"new_key_hash": &record.key_hash})),
                    metadata: None,
                },
            )
            .await;
            Ok(Json(record))
        }
        None => Err(AppError::Auth("auth store not initialized".into())),
    }
}

// ── Quota management (Module A2 — v2.0) ───────────────────────────────────

pub async fn admin_api_quotas(
    State(state): State<crate::server::AppState>,
) -> Result<Json<Vec<crate::quota::TenantQuotaSnapshot>>, AppError> {
    match &state.quota_tracker {
        Some(tracker) => {
            let snapshots = tracker.usage_snapshot();
            Ok(Json(snapshots))
        }
        None => Err(AppError::Auth("quota tracker not initialized".into())),
    }
}

// ── v4.0 Track H: Config-as-Data CRUD (T176) ────────────────────────────

fn config_store(state: &crate::server::AppState) -> Result<&std::sync::Arc<ConfigStore>, AppError> {
    state
        .config_store
        .as_ref()
        .ok_or_else(|| AppError::Config("config store not initialized".into()))
}

// ── Provider config CRUD ──────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ProviderListResponse {
    pub providers: Vec<serde_json::Value>,
}

pub async fn admin_api_list_providers(
    State(state): State<crate::server::AppState>,
) -> Result<Json<ProviderListResponse>, AppError> {
    let store = config_store(&state)?;
    let providers = store.get_providers().await;
    let list: Vec<serde_json::Value> = providers
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "kind": format!("{:?}", p.kind).to_lowercase(),
                "base_url": p.base_url,
                "pool_id": p.pool_id,
                "api_version": p.api_version,
                "region": p.region,
                "metadata": p.metadata,
            })
        })
        .collect();
    Ok(Json(ProviderListResponse { providers: list }))
}

#[derive(serde::Deserialize, Serialize)]
pub struct CreateProviderRequest {
    pub id: String,
    pub kind: String,
    pub base_url: String,
    pub pool_id: String,
    #[serde(default)]
    pub api_version: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

pub async fn admin_api_create_provider(
    State(state): State<crate::server::AppState>,
    Json(req): Json<CreateProviderRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;
    let metadata_str = serde_json::to_string(&req.metadata).unwrap_or_default();

    sqlx::query(
        "INSERT INTO provider_config (id, kind, base_url, pool_id, metadata) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&req.id)
    .bind(&req.kind)
    .bind(&req.base_url)
    .bind(&req.pool_id)
    .bind(&metadata_str)
    .execute(store.db())
    .await
    .map_err(|e| AppError::Internal(format!("create provider: {e}")))?;

    let _ = audit_trail::record_audit(
        store.db(),
        audit_trail::AuditEvent {
            event_type: "provider.create".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "provider_config".into(),
            target_id: req.id.clone(),
            before_json: None,
            after_json: Some(serde_json::to_value(&req).unwrap_or_default()),
            metadata: None,
        },
    )
    .await;

    // Trigger immediate cache refresh
    let _ = store.refresh_from_db().await;

    Ok(Json(serde_json::json!({"ok": true, "id": req.id})))
}

pub async fn admin_api_delete_provider(
    State(state): State<crate::server::AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;

    sqlx::query("DELETE FROM provider_config WHERE id = ?1")
        .bind(&provider_id)
        .execute(store.db())
        .await
        .map_err(|e| AppError::Internal(format!("delete provider: {e}")))?;

    let _ = audit_trail::record_audit(
        store.db(),
        audit_trail::AuditEvent {
            event_type: "provider.delete".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "provider_config".into(),
            target_id: provider_id.clone(),
            before_json: Some(serde_json::json!({"id": &provider_id})),
            after_json: None,
            metadata: None,
        },
    )
    .await;

    let _ = store.refresh_from_db().await;

    Ok(Json(
        serde_json::json!({"ok": true, "deleted": provider_id}),
    ))
}

// ── Key pool CRUD ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PoolListResponse {
    pub pools: Vec<serde_json::Value>,
}

pub async fn admin_api_list_pools(
    State(state): State<crate::server::AppState>,
) -> Result<Json<PoolListResponse>, AppError> {
    let store = config_store(&state)?;
    let pools = store.get_pools().await;
    let list: Vec<serde_json::Value> = pools
        .iter()
        .map(|(id, cfg)| {
            serde_json::json!({
                "id": id,
                "strategy": format!("{:?}", cfg.strategy).to_lowercase(),
                "key_count": cfg.keys.len(),
                "keys": cfg.keys.iter().map(|k| serde_json::json!({
                    "key_hash": crate::db::compute_key_hash(&k.key),
                    "weight": k.weight,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(PoolListResponse { pools: list }))
}

#[derive(serde::Deserialize, Serialize)]
pub struct CreatePoolRequest {
    pub id: String,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default)]
    pub keys: Vec<CreateKeyEntryRequest>,
}

fn default_strategy() -> String {
    "weighted_random".into()
}

#[derive(serde::Deserialize, Serialize)]
pub struct CreateKeyEntryRequest {
    pub key: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
}

fn default_weight() -> u32 {
    1
}

pub async fn admin_api_create_pool(
    State(state): State<crate::server::AppState>,
    Json(req): Json<CreatePoolRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;

    sqlx::query("INSERT INTO key_pool (id, strategy) VALUES (?1, ?2)")
        .bind(&req.id)
        .bind(&req.strategy)
        .execute(store.db())
        .await
        .map_err(|e| AppError::Internal(format!("create pool: {e}")))?;

    for ke in &req.keys {
        let kh = crate::db::compute_key_hash(&ke.key);
        sqlx::query(
            "INSERT INTO key_entry (pool_id, key_hash, key_plain, weight) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(&req.id)
        .bind(&kh)
        .bind(&ke.key)
        .bind(ke.weight as i64)
        .execute(store.db())
        .await
        .map_err(|e| AppError::Internal(format!("create pool key: {e}")))?;
    }

    let _ = audit_trail::record_audit(
        store.db(),
        audit_trail::AuditEvent {
            event_type: "pool.create".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "key_pool".into(),
            target_id: req.id.clone(),
            before_json: None,
            after_json: Some(serde_json::to_value(&req).unwrap_or_default()),
            metadata: None,
        },
    )
    .await;

    let _ = store.refresh_from_db().await;

    Ok(Json(serde_json::json!({"ok": true, "id": req.id})))
}

pub async fn admin_api_delete_pool(
    State(state): State<crate::server::AppState>,
    Path(pool_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;

    // Delete keys first (CASCADE should handle this, but be explicit)
    sqlx::query("DELETE FROM key_entry WHERE pool_id = ?1")
        .bind(&pool_id)
        .execute(store.db())
        .await
        .map_err(|e| AppError::Internal(format!("delete pool keys: {e}")))?;

    sqlx::query("DELETE FROM key_pool WHERE id = ?1")
        .bind(&pool_id)
        .execute(store.db())
        .await
        .map_err(|e| AppError::Internal(format!("delete pool: {e}")))?;

    let _ = audit_trail::record_audit(
        store.db(),
        audit_trail::AuditEvent {
            event_type: "pool.delete".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "key_pool".into(),
            target_id: pool_id.clone(),
            before_json: Some(serde_json::json!({"id": &pool_id})),
            after_json: None,
            metadata: None,
        },
    )
    .await;

    let _ = store.refresh_from_db().await;

    Ok(Json(serde_json::json!({"ok": true, "deleted": pool_id})))
}

// ── Routing config CRUD ───────────────────────────────────────────────────

pub async fn admin_api_list_routing(
    State(state): State<crate::server::AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;
    let routing = store.get_model_routing().await;

    let entries: Vec<serde_json::Value> = routing
        .iter()
        .map(|(model, r)| {
            serde_json::json!({
                "logical_model": model,
                "pool_id": r.pool_id(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({"routing": entries})))
}

#[derive(serde::Deserialize, Serialize)]
pub struct CreateRoutingRequest {
    pub logical_model: String,
    pub pool_id: String,
    #[serde(default)]
    pub default_params: Option<serde_json::Value>,
}

pub async fn admin_api_create_routing(
    State(state): State<crate::server::AppState>,
    Json(req): Json<CreateRoutingRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;
    let params_str = req
        .default_params
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());

    sqlx::query(
        "INSERT OR REPLACE INTO routing_config (logical_model, pool_id, default_params) VALUES (?1, ?2, ?3)",
    )
    .bind(&req.logical_model)
    .bind(&req.pool_id)
    .bind(params_str.as_deref())
    .execute(store.db())
    .await
    .map_err(|e| AppError::Internal(format!("create routing: {e}")))?;

    let _ = audit_trail::record_audit(
        store.db(),
        audit_trail::AuditEvent {
            event_type: "routing.create".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "routing_config".into(),
            target_id: req.logical_model.clone(),
            before_json: None,
            after_json: Some(serde_json::to_value(&req).unwrap_or_default()),
            metadata: None,
        },
    )
    .await;

    let _ = store.refresh_from_db().await;

    Ok(Json(
        serde_json::json!({"ok": true, "model": req.logical_model}),
    ))
}

pub async fn admin_api_delete_routing(
    State(state): State<crate::server::AppState>,
    Path(logical_model): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;

    sqlx::query("DELETE FROM routing_config WHERE logical_model = ?1")
        .bind(&logical_model)
        .execute(store.db())
        .await
        .map_err(|e| AppError::Internal(format!("delete routing: {e}")))?;

    let _ = audit_trail::record_audit(
        store.db(),
        audit_trail::AuditEvent {
            event_type: "routing.delete".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "routing_config".into(),
            target_id: logical_model.clone(),
            before_json: Some(serde_json::json!({"logical_model": &logical_model})),
            after_json: None,
            metadata: None,
        },
    )
    .await;

    let _ = store.refresh_from_db().await;

    Ok(Json(
        serde_json::json!({"ok": true, "deleted": logical_model}),
    ))
}

// ── Config rollback (T173) ────────────────────────────────────────────────

pub async fn admin_api_rollback_config(
    State(state): State<crate::server::AppState>,
    Path(audit_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;

    // Look up the audit event
    let event: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT event_type, target_type, before_json FROM audit_trail WHERE id = ?1",
    )
    .bind(&audit_id)
    .fetch_optional(store.db())
    .await
    .map_err(|e| AppError::Internal(format!("lookup audit: {e}")))?;

    let (event_type, target_type, before_json) =
        event.ok_or_else(|| AppError::NotFound(format!("audit event {audit_id} not found")))?;

    let before: serde_json::Value = before_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);

    // Revert based on target type
    match (target_type.as_str(), event_type.as_str()) {
        ("provider_config", "provider.create") | ("provider_config", "provider.update") => {
            // For create/update, delete the config to revert
            if let Some(id) = before.get("id").and_then(|v| v.as_str()) {
                sqlx::query("DELETE FROM provider_config WHERE id = ?1")
                    .bind(id)
                    .execute(store.db())
                    .await
                    .map_err(|e| AppError::Internal(format!("rollback provider: {e}")))?;
            }
        }
        ("provider_config", "provider.delete") => {
            // For delete, re-insert from before_json
            if let (Some(id), Some(kind), Some(base_url), Some(pool_id)) = (
                before.get("id").and_then(|v| v.as_str()),
                before.get("kind").and_then(|v| v.as_str()),
                before.get("base_url").and_then(|v| v.as_str()),
                before.get("pool_id").and_then(|v| v.as_str()),
            ) {
                sqlx::query(
                    "INSERT OR IGNORE INTO provider_config (id, kind, base_url, pool_id) VALUES (?1, ?2, ?3, ?4)",
                )
                .bind(id)
                .bind(kind)
                .bind(base_url)
                .bind(pool_id)
                .execute(store.db())
                .await
                .map_err(|e| AppError::Internal(format!("rollback provider: {e}")))?;
            }
        }
        // ── AUDIT-16 Fix: key_pool rollback ──
        ("key_pool", "pool.create") => {
            if let Some(id) = before.get("id").and_then(|v| v.as_str()) {
                sqlx::query("DELETE FROM key_entry WHERE pool_id = ?1")
                    .bind(id)
                    .execute(store.db())
                    .await
                    .map_err(|e| AppError::Internal(format!("rollback pool keys: {e}")))?;
                sqlx::query("DELETE FROM key_pool WHERE id = ?1")
                    .bind(id)
                    .execute(store.db())
                    .await
                    .map_err(|e| AppError::Internal(format!("rollback pool: {e}")))?;
            }
        }
        ("key_pool", "pool.delete") => {
            if let Some(id) = before.get("id").and_then(|v| v.as_str()) {
                sqlx::query(
                    "INSERT OR IGNORE INTO key_pool (id, strategy) VALUES (?1, 'weighted_random')",
                )
                .bind(id)
                .execute(store.db())
                .await
                .map_err(|e| AppError::Internal(format!("rollback pool: {e}")))?;
            }
        }
        // ── AUDIT-16 Fix: routing_config rollback ──
        ("routing_config", "routing.create") => {
            if let Some(model) = before.get("logical_model").and_then(|v| v.as_str()) {
                sqlx::query("DELETE FROM routing_config WHERE logical_model = ?1")
                    .bind(model)
                    .execute(store.db())
                    .await
                    .map_err(|e| AppError::Internal(format!("rollback routing: {e}")))?;
            }
        }
        ("routing_config", "routing.delete") => {
            if let (Some(model), Some(pool_id)) = (
                before.get("logical_model").and_then(|v| v.as_str()),
                before.get("pool_id").and_then(|v| v.as_str()),
            ) {
                sqlx::query(
                    "INSERT OR REPLACE INTO routing_config (logical_model, pool_id) VALUES (?1, ?2)",
                )
                .bind(model)
                .bind(pool_id)
                .execute(store.db())
                .await
                .map_err(|e| AppError::Internal(format!("rollback routing: {e}")))?;
            }
        }
        ("key_pool", _) | ("routing_config", _) => {
            return Err(AppError::Config(format!(
                "rollback not supported for event type {event_type} on {target_type}"
            )));
        }
        _ => {
            return Err(AppError::Config(format!(
                "unknown target type for rollback: {target_type}"
            )));
        }
    }

    let _ = audit_trail::record_audit(
        store.db(),
        audit_trail::AuditEvent {
            event_type: "config.rollback".into(),
            actor_id: None,
            actor_ip: None,
            target_type: target_type.clone(),
            target_id: audit_id.clone(),
            before_json: Some(serde_json::json!({"rolled_back_audit_id": &audit_id})),
            after_json: None,
            metadata: None,
        },
    )
    .await;

    let _ = store.refresh_from_db().await;

    Ok(Json(
        serde_json::json!({"ok": true, "rolled_back": audit_id}),
    ))
}

// ── Budget validation endpoint (AUDIT-15 Fix) ───────────────────────────

#[derive(serde::Deserialize)]
pub struct ValidateBudgetRequest {
    pub parent_scope_type: String, // "organization" | "team"
    pub parent_scope_id: String,
    pub proposed_child_budget: Option<f64>,
}

pub async fn admin_api_validate_budget(
    State(state): State<crate::server::AppState>,
    Json(req): Json<ValidateBudgetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bm = state
        .budget_manager
        .as_ref()
        .ok_or_else(|| AppError::Config("budget manager not initialized".into()))?;

    bm.validate_budget_inheritance(
        &req.parent_scope_type,
        &req.parent_scope_id,
        req.proposed_child_budget,
    )
    .await?;

    Ok(Json(serde_json::json!({"ok": true})))
}

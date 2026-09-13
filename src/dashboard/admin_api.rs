//! Admin REST API — endpoints for external management tools (v2.0).
//!
//! Client-key management is an ephemeral runtime carrier. CRUD changes are
//! not persisted because the metadata-only database table cannot recover keys.

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

// ── Standard status / keys ───────────────────────────────────────────────

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

#[derive(Serialize)]
pub struct ConfigRefreshResponse {
    pub ok: bool,
    pub version: u64,
}

#[derive(Serialize)]
struct ExportPool {
    keys: Vec<ExportKey>,
    strategy: String,
}

#[derive(Serialize)]
struct ExportKey {
    key: String,
    weight: u32,
    #[serde(rename = "type", skip_serializing_if = "is_api_key_type")]
    cred_type: crate::config::CredentialType,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issuer: Option<String>,
}

fn is_api_key_type(t: &crate::config::CredentialType) -> bool {
    *t == crate::config::CredentialType::ApiKey
}

#[derive(Serialize)]
struct ExportServer {
    host: String,
    port: u16,
    max_body_bytes: usize,
}

#[derive(Serialize)]
struct ExportAuth {
    client_keys: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct ExportDb {
    path: String,
}

#[derive(Serialize)]
struct ExportFailover {
    enabled: bool,
    bad_status_codes: Vec<u16>,
    max_retries: u32,
    probe_interval_secs: u64,
    probe_timeout_secs: u64,
    max_probe_retries: u32,
}

#[derive(Serialize)]
struct ExportDocument {
    server: ExportServer,
    auth: ExportAuth,
    db: ExportDb,
    failover: ExportFailover,
    providers: Vec<serde_json::Value>,
    pools: std::collections::BTreeMap<String, ExportPool>,
    model_to_pool: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    model_registry: Vec<serde_json::Value>,
}

fn pool_strategy_to_yaml(strategy: &crate::config::PoolStrategy) -> &'static str {
    match strategy {
        crate::config::PoolStrategy::WeightedRandom => "weighted_random",
    }
}

fn provider_kind_to_yaml(kind: &crate::config::ProviderKind) -> &'static str {
    // Keep export names identical to ConfigStore/DB (`openai`), not serde's
    // default `open_ai`, so export → validate → import round-trips cleanly.
    match kind {
        crate::config::ProviderKind::OpenAi => "openai",
        crate::config::ProviderKind::Anthropic => "anthropic",
        crate::config::ProviderKind::Gemini => "gemini",
        crate::config::ProviderKind::Azure => "azure",
        crate::config::ProviderKind::Bedrock => "bedrock",
        crate::config::ProviderKind::Cohere => "cohere",
        crate::config::ProviderKind::Mistral => "mistral",
        crate::config::ProviderKind::Ollama => "ollama",
        crate::config::ProviderKind::Vllm => "vllm",
    }
}

pub async fn admin_api_config_export(
    State(state): State<crate::server::AppState>,
) -> Result<axum::response::Response, AppError> {
    let snapshot = state.config_store.snapshot().await;
    let pools = snapshot
        .pool_configs
        .iter()
        .map(|(id, pool)| {
            (
                id.clone(),
                ExportPool {
                    strategy: pool_strategy_to_yaml(&pool.strategy).into(),
                    keys: pool
                        .keys
                        .iter()
                        .map(|key| ExportKey {
                            key: key.key.clone(),
                            weight: key.weight,
                            cred_type: key.cred_type,
                            refresh: key.refresh.clone(),
                            expires: key.expires,
                            issuer: key.issuer.clone(),
                        })
                        .collect(),
                },
            )
        })
        .collect();
    let providers = snapshot
        .providers
        .iter()
        .map(|provider| {
            serde_json::json!({
                "id": provider.id,
                "kind": provider_kind_to_yaml(&provider.kind),
                "base_url": provider.base_url,
                "pool_id": provider.pool_id,
                "api_version": provider.api_version,
                "region": provider.region,
                "metadata": provider.metadata,
            })
        })
        .collect();
    let model_to_pool = snapshot
        .model_routing
        .iter()
        .map(|(model, routing)| {
            let value = match routing {
                crate::config::ModelRouting::Simple(pool) => {
                    serde_json::Value::String(pool.clone())
                }
                crate::config::ModelRouting::WithParams {
                    pool,
                    default_params,
                } => serde_json::json!({
                    "pool": pool,
                    "default_params": default_params,
                }),
            };
            (model.clone(), value)
        })
        .collect();
    let model_registry = snapshot
        .model_registry
        .iter()
        .map(|entry| {
            let capabilities = entry
                .capabilities_json
                .as_deref()
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            serde_json::json!({
                "id": entry.id,
                "display_name": entry.display_name,
                "provider_kind": entry.provider_kind,
                "provider_config_id": entry.provider_config_id,
                "supports_vision": entry.supports_vision,
                "supports_tool_calling": entry.supports_tool_calling,
                "supports_json_mode": entry.supports_json_mode,
                "max_context_tokens": entry.max_context_tokens,
                "max_output_tokens": entry.max_output_tokens,
                "input_price_per_1m": entry.input_price_per_1m,
                "output_price_per_1m": entry.output_price_per_1m,
                "capabilities_json": capabilities,
                "enabled": entry.enabled,
            })
        })
        .collect();
    let runtime = &snapshot.runtime.failover;
    let yaml = serde_yaml::to_string(&ExportDocument {
        server: ExportServer {
            host: state.config.server.host.clone(),
            port: state.config.server.port,
            max_body_bytes: state.config.server.max_body_bytes,
        },
        // Client keys stay bootstrap-only (ADR-017): hashes are not reversible,
        // so export never fabricates plaintext auth material.
        auth: ExportAuth {
            client_keys: Vec::new(),
        },
        db: ExportDb {
            path: state.config.db.path.clone(),
        },
        failover: ExportFailover {
            enabled: runtime.enabled,
            bad_status_codes: runtime.bad_status_codes.clone(),
            max_retries: runtime.max_retries,
            probe_interval_secs: runtime.probe_interval_secs,
            probe_timeout_secs: runtime.probe_timeout_secs,
            max_probe_retries: runtime.max_probe_retries,
        },
        providers,
        pools,
        model_to_pool,
        model_registry,
    })
    .map_err(|e| AppError::Internal(format!("export config: {e}")))?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/yaml; charset=utf-8"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=llm-proxy-config.yaml",
            ),
        ],
        yaml,
    )
        .into_response())
}
#[derive(Serialize)]
pub struct ConfigOverviewResponse {
    pub version: u64,
    pub providers: usize,
    pub pools: usize,
    pub routing: usize,
    pub model_registry: usize,
}

#[derive(Deserialize)]
pub struct ConfigDocumentRequest {
    pub yaml: String,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize)]
pub struct ConfigDocumentResponse {
    pub ok: bool,
    pub dry_run: bool,
    pub version: u64,
    pub message: String,
}

pub async fn admin_api_config_overview(
    State(state): State<crate::server::AppState>,
) -> Result<Json<ConfigOverviewResponse>, AppError> {
    let snapshot = state.config_store.snapshot().await;
    Ok(Json(ConfigOverviewResponse {
        version: snapshot.version,
        providers: snapshot.providers.len(),
        pools: snapshot.pool_configs.len(),
        routing: snapshot.model_routing.len(),
        model_registry: snapshot.model_registry.len(),
    }))
}

pub async fn admin_api_config_validate(
    Json(req): Json<ConfigDocumentRequest>,
) -> Result<Json<ConfigDocumentResponse>, AppError> {
    ConfigStore::validate_yaml(&req.yaml)?;
    Ok(Json(ConfigDocumentResponse {
        ok: true,
        dry_run: true,
        version: 0,
        message: "configuration is valid; no changes were made".into(),
    }))
}

pub async fn admin_api_config_import(
    State(state): State<crate::server::AppState>,
    Json(req): Json<ConfigDocumentRequest>,
) -> Result<Json<ConfigDocumentResponse>, AppError> {
    ConfigStore::validate_yaml(&req.yaml)?;
    if req.dry_run {
        return Ok(Json(ConfigDocumentResponse {
            ok: true,
            dry_run: true,
            version: state.config_store.version().await,
            message: "configuration is valid; no changes were made".into(),
        }));
    }
    state.config_store.import_yaml(&req.yaml).await?;
    let version = state.config_store.version().await;
    let _ = audit_trail::record_audit(
        &state.db,
        audit_trail::AuditEvent {
            event_type: "config.import".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "config_store".into(),
            target_id: version.to_string(),
            before_json: None,
            after_json: Some(serde_json::json!({ "version": version })),
            metadata: Some(serde_json::json!({ "source": "explicit_yaml" })),
        },
    )
    .await;
    Ok(Json(ConfigDocumentResponse {
        ok: true,
        dry_run: false,
        version,
        message: "configuration imported".into(),
    }))
}

pub async fn admin_api_config_refresh(
    State(state): State<crate::server::AppState>,
) -> Result<Json<ConfigRefreshResponse>, AppError> {
    state.config_store.refresh_from_db().await?;
    Ok(Json(ConfigRefreshResponse {
        ok: true,
        version: state.config_store.version().await,
    }))
}

#[derive(Serialize)]
pub struct ClientKeyListResponse {
    pub keys: Vec<crate::auth_store::ClientKeyPublicRecord>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ModelRegistryRecord {
    pub id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub provider_config_id: Option<String>,
    pub supports_vision: i32,
    pub supports_tool_calling: i32,
    pub supports_json_mode: i32,
    pub max_context_tokens: i32,
    pub max_output_tokens: i32,
    pub input_price_per_1m: Option<f64>,
    pub output_price_per_1m: Option<f64>,
    pub capabilities_json: Option<String>,
    pub enabled: i32,
}

#[derive(Debug, Deserialize)]
pub struct ModelRegistryCreate {
    pub id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub provider_config_id: Option<String>,
    #[serde(default)]
    pub supports_vision: bool,
    #[serde(default)]
    pub supports_tool_calling: bool,
    #[serde(default)]
    pub supports_json_mode: bool,
    #[serde(default = "default_model_tokens")]
    pub max_context_tokens: i32,
    #[serde(default = "default_model_tokens")]
    pub max_output_tokens: i32,
    pub input_price_per_1m: Option<f64>,
    pub output_price_per_1m: Option<f64>,
    pub capabilities_json: Option<Value>,
    #[serde(default = "default_model_enabled")]
    pub enabled: bool,
}
#[derive(Debug, Deserialize)]
pub struct ModelRegistryPatch {
    pub display_name: Option<String>,
    pub provider_kind: Option<String>,
    pub provider_config_id: Option<Option<String>>,
    pub supports_vision: Option<bool>,
    pub supports_tool_calling: Option<bool>,
    pub supports_json_mode: Option<bool>,
    pub max_context_tokens: Option<i32>,
    pub max_output_tokens: Option<i32>,
    pub input_price_per_1m: Option<Option<f64>>,
    pub output_price_per_1m: Option<Option<f64>>,
    pub capabilities_json: Option<Option<Value>>,
    pub enabled: Option<bool>,
}
fn default_model_tokens() -> i32 {
    4096
}
fn default_model_enabled() -> bool {
    true
}
struct ModelValidation<'a> {
    id: &'a str,
    display: &'a str,
    kind: &'a str,
    context: i32,
    output: i32,
    input: Option<f64>,
    output_price: Option<f64>,
    caps: Option<&'a Value>,
}

fn validate_model_fields(fields: ModelValidation<'_>) -> Result<(), AppError> {
    let ModelValidation {
        id,
        display,
        kind,
        context,
        output,
        input,
        output_price,
        caps,
    } = fields;
    if id.trim().is_empty() || display.trim().is_empty() || kind.trim().is_empty() {
        return Err(AppError::BadRequest(
            "id, display_name and provider_kind are required".into(),
        ));
    }
    if !matches!(
        kind,
        "openai"
            | "anthropic"
            | "gemini"
            | "azure"
            | "bedrock"
            | "cohere"
            | "mistral"
            | "ollama"
            | "vllm"
    ) {
        return Err(AppError::BadRequest("unsupported provider_kind".into()));
    }
    if context <= 0 || output <= 0 {
        return Err(AppError::BadRequest("token limits must be positive".into()));
    }
    if [input, output_price]
        .into_iter()
        .flatten()
        .any(|p| !p.is_finite() || p < 0.0)
    {
        return Err(AppError::BadRequest(
            "prices must be finite and non-negative".into(),
        ));
    }
    if caps.is_some_and(|v| !v.is_object()) {
        return Err(AppError::BadRequest(
            "capabilities_json must be a JSON object".into(),
        ));
    }
    Ok(())
}
async fn get_model(pool: &sqlx::SqlitePool, id: &str) -> Result<ModelRegistryRecord, AppError> {
    sqlx::query_as::<_, ModelRegistryRecord>("SELECT id, display_name, provider_kind, provider_config_id, supports_vision, supports_tool_calling, supports_json_mode, max_context_tokens, max_output_tokens, input_price_per_1m, output_price_per_1m, capabilities_json, enabled FROM model_registry WHERE id=?1").bind(id).fetch_optional(pool).await.map_err(|e| AppError::Internal(format!("load model: {e}")))?.ok_or_else(|| AppError::NotFound(format!("model '{id}' not found")))
}
fn model_value(r: &ModelRegistryRecord) -> Value {
    serde_json::json!({"id":r.id,"display_name":r.display_name,"provider_kind":r.provider_kind,"provider_config_id":r.provider_config_id,"supports_vision":r.supports_vision != 0,"supports_tool_calling":r.supports_tool_calling != 0,"supports_json_mode":r.supports_json_mode != 0,"max_context_tokens":r.max_context_tokens,"max_output_tokens":r.max_output_tokens,"input_price_per_1m":r.input_price_per_1m,"output_price_per_1m":r.output_price_per_1m,"capabilities_json":r.capabilities_json.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_else(|| serde_json::json!({})),"enabled":r.enabled != 0})
}
async fn validate_provider(pool: &sqlx::SqlitePool, id: Option<&str>) -> Result<(), AppError> {
    if let Some(id) = id {
        provider_pool_id(pool, id).await?;
    }
    Ok(())
}

/// Resolve an enabled provider's pool so a registry row can become callable.
async fn provider_pool_id(pool: &sqlx::SqlitePool, provider_id: &str) -> Result<String, AppError> {
    if provider_id.trim().is_empty() {
        return Err(AppError::BadRequest(
            "provider_config_id cannot be empty".into(),
        ));
    }
    let pool_id: Option<String> =
        sqlx::query_scalar("SELECT pool_id FROM provider_config WHERE id=?1 AND enabled = 1")
            .bind(provider_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::Internal(format!("validate provider: {e}")))?;
    pool_id.ok_or_else(|| {
        AppError::BadRequest(format!(
            "provider_config_id '{provider_id}' does not exist or is disabled"
        ))
    })
}

/// Ensure a logical model is routed through the provider's pool so it appears
/// in `/v1/models` and is callable by CLI clients.
async fn ensure_model_routing(
    pool: &sqlx::SqlitePool,
    logical_model: &str,
    provider_config_id: Option<&str>,
    enabled: bool,
) -> Result<(), AppError> {
    let Some(provider_id) = provider_config_id.filter(|id| !id.trim().is_empty()) else {
        return Ok(());
    };
    if !enabled {
        return Ok(());
    }
    let pool_id = provider_pool_id(pool, provider_id).await?;
    let pool_exists: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM key_pool WHERE id = ?1 AND enabled = 1")
            .bind(&pool_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::Internal(format!("validate routing pool: {e}")))?;
    if pool_exists.is_none() {
        return Err(AppError::BadRequest(format!(
            "provider '{provider_id}' references missing or disabled pool '{pool_id}'"
        )));
    }
    sqlx::query("DELETE FROM routing_config WHERE logical_model = ?1")
        .bind(logical_model)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(format!("ensure model routing: {e}")))?;
    sqlx::query(
        "INSERT INTO routing_config (logical_model, pool_id, default_params, enabled)
         VALUES (?1, ?2, NULL, 1)",
    )
    .bind(logical_model)
    .bind(pool_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(format!("ensure model routing: {e}")))?;
    Ok(())
}

pub async fn admin_api_models(
    State(state): State<crate::server::AppState>,
) -> Result<Json<Value>, AppError> {
    let rows = sqlx::query_as::<_, ModelRegistryRecord>("SELECT id, display_name, provider_kind, provider_config_id, supports_vision, supports_tool_calling, supports_json_mode, max_context_tokens, max_output_tokens, input_price_per_1m, output_price_per_1m, capabilities_json, enabled FROM model_registry ORDER BY id").fetch_all(&state.db).await.map_err(|e| AppError::Internal(format!("list models: {e}")))?;
    Ok(Json(
        serde_json::json!({"models": rows.iter().map(model_value).collect::<Vec<_>>() }),
    ))
}

pub async fn admin_api_create_model(
    State(state): State<crate::server::AppState>,
    Json(req): Json<ModelRegistryCreate>,
) -> Result<Json<Value>, AppError> {
    validate_model_fields(ModelValidation {
        id: &req.id,
        display: &req.display_name,
        kind: &req.provider_kind,
        context: req.max_context_tokens,
        output: req.max_output_tokens,
        input: req.input_price_per_1m,
        output_price: req.output_price_per_1m,
        caps: req.capabilities_json.as_ref(),
    })?;
    validate_provider(&state.db, req.provider_config_id.as_deref()).await?;
    let result = sqlx::query("INSERT INTO model_registry (id,display_name,provider_kind,provider_config_id,supports_vision,supports_tool_calling,supports_json_mode,max_context_tokens,max_output_tokens,input_price_per_1m,output_price_per_1m,capabilities_json,enabled) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)").bind(&req.id).bind(&req.display_name).bind(&req.provider_kind).bind(&req.provider_config_id).bind(req.supports_vision as i32).bind(req.supports_tool_calling as i32).bind(req.supports_json_mode as i32).bind(req.max_context_tokens).bind(req.max_output_tokens).bind(req.input_price_per_1m).bind(req.output_price_per_1m).bind(req.capabilities_json.as_ref().map(Value::to_string)).bind(req.enabled as i32).execute(&state.db).await;
    if let Err(e) = result {
        if e.as_database_error()
            .is_some_and(|d| d.is_unique_violation())
        {
            return Err(AppError::BadRequest("model id already exists".into()));
        }
        return Err(AppError::Internal(format!("create model: {e}")));
    }
    if let Err(error) = ensure_model_routing(
        &state.db,
        &req.id,
        req.provider_config_id.as_deref(),
        req.enabled,
    )
    .await
    {
        let _ = sqlx::query("DELETE FROM model_registry WHERE id = ?1")
            .bind(&req.id)
            .execute(&state.db)
            .await;
        return Err(error);
    }
    let after = get_model(&state.db, &req.id).await?;
    audit_trail::record_audit(
        &state.db,
        audit_trail::AuditEvent {
            event_type: "model.create".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "model_registry".into(),
            target_id: req.id,
            before_json: None,
            after_json: Some(model_value(&after)),
            metadata: None,
        },
    )
    .await?;
    state.config_store.refresh_from_db().await?;
    Ok(Json(model_value(&after)))
}

pub async fn admin_api_update_model(
    State(state): State<crate::server::AppState>,
    Path(id): Path<String>,
    Json(req): Json<ModelRegistryPatch>,
) -> Result<Json<Value>, AppError> {
    let before = get_model(&state.db, &id).await?;
    let display = req.display_name.as_deref().unwrap_or(&before.display_name);
    let kind = req
        .provider_kind
        .as_deref()
        .unwrap_or(&before.provider_kind);
    let context = req.max_context_tokens.unwrap_or(before.max_context_tokens);
    let output = req.max_output_tokens.unwrap_or(before.max_output_tokens);
    let input = req.input_price_per_1m.unwrap_or(before.input_price_per_1m);
    let out_price = req
        .output_price_per_1m
        .unwrap_or(before.output_price_per_1m);
    let provider = req
        .provider_config_id
        .as_ref()
        .map(|v| v.as_deref())
        .unwrap_or(before.provider_config_id.as_deref());
    let caps = req
        .capabilities_json
        .as_ref()
        .map(|v| v.as_ref().map(Value::to_string))
        .unwrap_or_else(|| before.capabilities_json.clone());
    let caps_value = req.capabilities_json.as_ref().and_then(|v| v.as_ref());
    validate_model_fields(ModelValidation {
        id: &id,
        display,
        kind,
        context,
        output,
        input,
        output_price: out_price,
        caps: caps_value,
    })?;
    validate_provider(&state.db, provider).await?;
    let enabled = req.enabled.unwrap_or(before.enabled != 0);
    sqlx::query("UPDATE model_registry SET display_name=?1,provider_kind=?2,provider_config_id=?3,supports_vision=?4,supports_tool_calling=?5,supports_json_mode=?6,max_context_tokens=?7,max_output_tokens=?8,input_price_per_1m=?9,output_price_per_1m=?10,capabilities_json=?11,enabled=?12,updated_at=unixepoch('subsec')*1000 WHERE id=?13").bind(display).bind(kind).bind(provider).bind(req.supports_vision.unwrap_or(before.supports_vision != 0) as i32).bind(req.supports_tool_calling.unwrap_or(before.supports_tool_calling != 0) as i32).bind(req.supports_json_mode.unwrap_or(before.supports_json_mode != 0) as i32).bind(context).bind(output).bind(input).bind(out_price).bind(caps).bind(enabled as i32).bind(&id).execute(&state.db).await.map_err(|e|AppError::Internal(format!("update model: {e}")))?;
    ensure_model_routing(&state.db, &id, provider, enabled).await?;
    let after = get_model(&state.db, &id).await?;
    audit_trail::record_audit(
        &state.db,
        audit_trail::AuditEvent {
            event_type: "model.update".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "model_registry".into(),
            target_id: id,
            before_json: Some(model_value(&before)),
            after_json: Some(model_value(&after)),
            metadata: None,
        },
    )
    .await?;
    state.config_store.refresh_from_db().await?;
    Ok(Json(model_value(&after)))
}

pub async fn admin_api_list_client_keys(
    State(state): State<crate::server::AppState>,
) -> Result<Json<ClientKeyListResponse>, AppError> {
    match &state.auth_store {
        Some(store) => {
            let keys = store
                .list()
                .iter()
                .map(crate::auth_store::ClientKeyPublicRecord::from)
                .collect::<Vec<_>>();
            let total = keys.len();
            Ok(Json(ClientKeyListResponse { keys, total }))
        }
        None => Err(AppError::Auth("auth store not initialized".into())),
    }
}

pub async fn admin_api_add_client_key(
    State(state): State<crate::server::AppState>,
    Json(req): Json<crate::auth_store::CreateKeyRequest>,
) -> Result<Json<crate::auth_store::ClientKeyPublicRecord>, AppError> {
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
                    after_json: Some(
                        serde_json::to_value(crate::auth_store::ClientKeyPublicRecord::from(
                            &record,
                        ))
                        .unwrap_or_default(),
                    ),
                    metadata: None,
                },
            )
            .await;
            Ok(Json(crate::auth_store::ClientKeyPublicRecord::from(
                &record,
            )))
        }
        None => Err(AppError::Auth("auth store not initialized".into())),
    }
}

pub async fn admin_api_update_client_key(
    State(state): State<crate::server::AppState>,
    Path(key_hash): Path<String>,
    Json(req): Json<crate::auth_store::UpdateKeyRequest>,
) -> Result<Json<crate::auth_store::ClientKeyPublicRecord>, AppError> {
    match &state.auth_store {
        Some(store) => {
            let record = store.update(&key_hash, req)?;
            Ok(Json(crate::auth_store::ClientKeyPublicRecord::from(
                &record,
            )))
        }
        None => Err(AppError::Auth("auth store not initialized".into())),
    }
}

pub async fn admin_api_delete_client_key(
    State(state): State<crate::server::AppState>,
    Path(key_hash): Path<String>,
) -> Result<Json<crate::auth_store::ClientKeyPublicRecord>, AppError> {
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
                    before_json: before.as_ref().map(|r| {
                        serde_json::to_value(crate::auth_store::ClientKeyPublicRecord::from(r))
                            .unwrap_or_default()
                    }),
                    after_json: None,
                    metadata: None,
                },
            )
            .await;
            Ok(Json(crate::auth_store::ClientKeyPublicRecord::from(
                &record,
            )))
        }
        None => Err(AppError::Auth("auth store not initialized".into())),
    }
}

pub async fn admin_api_rotate_client_key(
    State(state): State<crate::server::AppState>,
    Path(key_hash): Path<String>,
    Json(req): Json<crate::auth_store::RotateKeyRequest>,
) -> Result<Json<crate::auth_store::ClientKeyPublicRecord>, AppError> {
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
            Ok(Json(crate::auth_store::ClientKeyPublicRecord::from(
                &record,
            )))
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
    Ok(&state.config_store)
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
    let providers = store.snapshot().await.providers;
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
    if req.id.trim().is_empty() {
        return Err(AppError::BadRequest("provider id must not be empty".into()));
    }
    if req.base_url.trim().is_empty() {
        return Err(AppError::BadRequest(
            "provider base_url must not be empty".into(),
        ));
    }
    let existing: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM provider_config WHERE id = ?1")
        .bind(&req.id)
        .fetch_one(store.db())
        .await
        .map_err(|e| AppError::Internal(format!("validate provider id: {e}")))?;
    if existing.0 != 0 {
        return Err(AppError::BadRequest(format!(
            "provider '{}' already exists",
            req.id
        )));
    }

    let mut metadata = req.metadata.clone();
    if !metadata.is_object() {
        return Err(AppError::BadRequest(
            "provider metadata must be a JSON object".into(),
        ));
    }
    if let Some(object) = metadata.as_object_mut() {
        if let Some(value) = &req.api_version {
            object.insert(
                "api_version".into(),
                serde_json::Value::String(value.clone()),
            );
        }
        if let Some(value) = &req.region {
            object.insert("region".into(), serde_json::Value::String(value.clone()));
        }
    }
    let kind = req.kind.to_ascii_lowercase();
    if !matches!(
        kind.as_str(),
        "openai"
            | "anthropic"
            | "gemini"
            | "azure"
            | "bedrock"
            | "cohere"
            | "mistral"
            | "ollama"
            | "vllm"
    ) {
        return Err(AppError::BadRequest(format!(
            "unsupported provider kind '{kind}'"
        )));
    }
    let pool_exists: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM key_pool WHERE id = ?1 AND enabled = 1")
            .bind(&req.pool_id)
            .fetch_one(store.db())
            .await
            .map_err(|e| AppError::Internal(format!("validate provider pool: {e}")))?;
    if pool_exists.0 == 0 {
        return Err(AppError::BadRequest(format!(
            "pool '{}' does not exist",
            req.pool_id
        )));
    }
    let metadata_str = serde_json::to_string(&metadata)
        .map_err(|e| AppError::Internal(format!("serialize provider metadata: {e}")))?;

    sqlx::query(
        "INSERT INTO provider_config (id, kind, base_url, pool_id, metadata) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&req.id)
    .bind(&kind)
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
            after_json: Some(serde_json::json!({
                "id": req.id,
                "kind": kind,
                "base_url": req.base_url,
                "pool_id": req.pool_id,
                "api_version": req.api_version,
                "region": req.region,
                "metadata": metadata.as_object().map(|object| object.iter().filter_map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    let sensitive = lower.contains("key") || lower.contains("token") || lower.contains("secret") || lower.contains("password") || lower.contains("credential") || lower.contains("authorization");
                    (!sensitive && value.is_string()).then(|| (key.clone(), value.clone()))
                }).collect::<std::collections::BTreeMap<_, _>>()).unwrap_or_default(),
            })),
            metadata: None,
        },
    )
    .await;

    // Trigger immediate cache refresh
    store.refresh_from_db().await?;

    Ok(Json(serde_json::json!({"ok": true, "id": req.id})))
}

pub async fn admin_api_delete_provider(
    State(state): State<crate::server::AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;
    let before: Option<String> = sqlx::query_scalar("SELECT json_object('id', id, 'kind', kind, 'base_url', base_url, 'pool_id', pool_id, 'metadata', metadata) FROM provider_config WHERE id = ?1")
        .bind(&provider_id)
        .fetch_optional(store.db())
        .await
        .map_err(|e| AppError::Internal(format!("load provider for audit: {e}")))?;

    let (before, has_sensitive_metadata) = before
        .and_then(|value| {
            let mut parsed = serde_json::from_str::<serde_json::Value>(&value).ok()?;
            if let Some(metadata) = parsed.get("metadata").and_then(|v| v.as_str()) {
                let metadata = serde_json::from_str(metadata)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                parsed["metadata"] = metadata;
            }
            Some(redact_audit_metadata(&parsed))
        })
        .map_or((None, false), |(value, sensitive)| (Some(value), sensitive));
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
            before_json: before,
            after_json: None,
            metadata: Some(serde_json::json!({
                "rollback_restricted": has_sensitive_metadata,
            })),
        },
    )
    .await;

    store.refresh_from_db().await?;

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
    let pools = store.snapshot().await.pool_configs;
    let list: Vec<serde_json::Value> = pools
        .iter()
        .map(|(id, cfg)| {
            serde_json::json!({
                "id": id,
                "strategy": format!("{:?}", cfg.strategy).to_lowercase(),
                "key_count": cfg.keys.len(),
                "keys": cfg.keys.iter().map(|k| serde_json::json!({
                    "key_hash": k.identity_hash(),
                    "weight": k.weight,
                    "type": k.cred_type_str(),
                    "issuer": k.issuer,
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
    #[serde(default)]
    pub key: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
    #[serde(default, rename = "type")]
    pub cred_type: crate::config::CredentialType,
    #[serde(default)]
    pub refresh: Option<String>,
    #[serde(default)]
    pub expires: Option<i64>,
    #[serde(default)]
    pub issuer: Option<String>,
}

impl CreateKeyEntryRequest {
    fn to_key_entry(&self) -> crate::config::KeyEntry {
        crate::config::KeyEntry {
            key: self.key.clone(),
            weight: self.weight,
            cred_type: self.cred_type,
            refresh: self.refresh.clone(),
            expires: self.expires,
            issuer: self.issuer.clone(),
            identity: None,
        }
    }
}

fn default_weight() -> u32 {
    1
}

pub async fn admin_api_create_pool(
    State(state): State<crate::server::AppState>,
    Json(req): Json<CreatePoolRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;
    let mut tx = store
        .db()
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("begin pool transaction: {e}")))?;
    sqlx::query("INSERT INTO key_pool (id, strategy) VALUES (?1, ?2)")
        .bind(&req.id)
        .bind(&req.strategy)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::BadRequest(format!("create pool: {e}")))?;
    for ke in &req.keys {
        let entry = ke.to_key_entry();
        if entry.weight == 0 {
            return Err(AppError::BadRequest(
                "pool keys must have positive weight".into(),
            ));
        }
        if entry.is_oauth() {
            if entry.refresh.as_deref().filter(|s| !s.is_empty()).is_none()
                || entry.issuer.as_deref().filter(|s| !s.is_empty()).is_none()
            {
                return Err(AppError::BadRequest(
                    "oauth pool keys require refresh and issuer".into(),
                ));
            }
        } else if entry.key.is_empty() {
            return Err(AppError::BadRequest(
                "pool keys must be non-empty with positive weight".into(),
            ));
        }
        crate::config_store::insert_key_entry(tx.as_mut(), &req.id, &entry)
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
    }
    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("commit pool transaction: {e}")))?;
    let after = serde_json::json!({
        "id": req.id,
        "strategy": req.strategy,
        "keys": req.keys.iter().map(|k| serde_json::json!({
            "key_hash": k.to_key_entry().identity_hash(),
            "weight": k.weight,
        })).collect::<Vec<_>>(),
    });
    let _ = audit_trail::record_audit(
        store.db(),
        audit_trail::AuditEvent {
            event_type: "pool.create".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "key_pool".into(),
            target_id: req.id.clone(),
            before_json: Some(serde_json::json!({"id": req.id})),
            after_json: Some(after),
            metadata: None,
        },
    )
    .await;
    store.refresh_from_db().await?;
    Ok(Json(serde_json::json!({"ok": true, "id": req.id})))
}

pub async fn admin_api_add_pool_key(
    State(state): State<crate::server::AppState>,
    Path(pool_id): Path<String>,
    Json(req): Json<CreateKeyEntryRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;
    let entry = req.to_key_entry();
    if entry.weight == 0 {
        return Err(AppError::BadRequest(
            "pool keys must have positive weight".into(),
        ));
    }
    if entry.is_oauth() {
        if entry.refresh.as_deref().filter(|s| !s.is_empty()).is_none()
            || entry.issuer.as_deref().filter(|s| !s.is_empty()).is_none()
        {
            return Err(AppError::BadRequest(
                "oauth pool keys require refresh and issuer".into(),
            ));
        }
    } else if entry.key.is_empty() {
        return Err(AppError::BadRequest("pool key must be non-empty".into()));
    }
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM key_pool WHERE id = ?1")
        .bind(&pool_id)
        .fetch_optional(store.db())
        .await
        .map_err(|e| AppError::Internal(format!("lookup pool: {e}")))?;
    if exists.is_none() {
        return Err(AppError::NotFound(format!("pool '{pool_id}' not found")));
    }
    let mut tx = store
        .db()
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("begin add key: {e}")))?;
    crate::config_store::insert_key_entry(tx.as_mut(), &pool_id, &entry)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("commit add key: {e}")))?;
    store.refresh_from_db().await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "pool_id": pool_id,
        "key_hash": entry.identity_hash(),
        "type": entry.cred_type_str(),
    })))
}

pub async fn admin_api_delete_pool(
    State(state): State<crate::server::AppState>,
    Path(pool_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;
    let before: Option<String> = sqlx::query_scalar("SELECT json_object('id', id, 'strategy', strategy, 'keys', (SELECT json_group_array(json_object('key_hash', key_hash, 'weight', weight)) FROM key_entry WHERE pool_id = key_pool.id)) FROM key_pool WHERE id = ?1")
        .bind(&pool_id)
        .fetch_optional(store.db()).await.map_err(|e| AppError::Internal(format!("load pool for audit: {e}")))?;
    let mut tx = store
        .db()
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("begin pool delete transaction: {e}")))?;
    sqlx::query("DELETE FROM key_entry WHERE pool_id = ?1")
        .bind(&pool_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("delete pool keys: {e}")))?;
    sqlx::query("DELETE FROM key_pool WHERE id = ?1")
        .bind(&pool_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("delete pool: {e}")))?;
    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("commit pool delete transaction: {e}")))?;
    let before = before.and_then(|v| serde_json::from_str(&v).ok());
    let _ = audit_trail::record_audit(
        store.db(),
        audit_trail::AuditEvent {
            event_type: "pool.delete".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "key_pool".into(),
            target_id: pool_id.clone(),
            before_json: before,
            after_json: None,
            metadata: None,
        },
    )
    .await;
    store.refresh_from_db().await?;
    Ok(Json(serde_json::json!({"ok": true, "deleted": pool_id})))
}

// ── Routing config CRUD ───────────────────────────────────────────────────

pub async fn admin_api_list_routing(
    State(state): State<crate::server::AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;
    let routing = store.snapshot().await.model_routing;

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
    let pool_exists: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM key_pool WHERE id = ?1 AND enabled = 1")
            .bind(&req.pool_id)
            .fetch_one(store.db())
            .await
            .map_err(|e| AppError::Internal(format!("validate routing pool: {e}")))?;
    if pool_exists.0 == 0 {
        return Err(AppError::BadRequest(format!(
            "pool '{}' does not exist",
            req.pool_id
        )));
    }
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

    store.refresh_from_db().await?;

    Ok(Json(
        serde_json::json!({"ok": true, "model": req.logical_model}),
    ))
}

pub async fn admin_api_delete_routing(
    State(state): State<crate::server::AppState>,
    Path(logical_model): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;
    let before: Option<String> = sqlx::query_scalar("SELECT json_object('logical_model', logical_model, 'pool_id', pool_id, 'default_params', default_params) FROM routing_config WHERE logical_model = ?1")
        .bind(&logical_model).fetch_optional(store.db()).await
        .map_err(|e| AppError::Internal(format!("load routing for audit: {e}")))?;

    let before = before.and_then(|value| serde_json::from_str(&value).ok());

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
            before_json: before,
            after_json: None,
            metadata: None,
        },
    )
    .await;

    store.refresh_from_db().await?;

    Ok(Json(
        serde_json::json!({"ok": true, "deleted": logical_model}),
    ))
}

fn redact_audit_metadata(value: &serde_json::Value) -> (serde_json::Value, bool) {
    let Some(object) = value.as_object() else {
        return (value.clone(), false);
    };
    let mut redacted = serde_json::Map::new();
    let mut sensitive = false;
    for (key, value) in object {
        let lower = key.to_ascii_lowercase();
        let is_sensitive = lower.contains("key")
            || lower.contains("token")
            || lower.contains("secret")
            || lower.contains("password")
            || lower.contains("credential")
            || lower.contains("authorization");
        if is_sensitive {
            redacted.insert(key.clone(), serde_json::Value::String("[REDACTED]".into()));
            sensitive = true;
        } else {
            let (clean, nested_sensitive) = redact_audit_metadata(value);
            redacted.insert(key.clone(), clean);
            sensitive |= nested_sensitive;
        }
    }
    (serde_json::Value::Object(redacted), sensitive)
}

fn contains_redacted(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => value == "[REDACTED]",
        serde_json::Value::Array(values) => values.iter().any(contains_redacted),
        serde_json::Value::Object(values) => values.values().any(contains_redacted),
        _ => false,
    }
}
pub async fn admin_api_rollback_config(
    State(state): State<crate::server::AppState>,
    Path(audit_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let store = config_store(&state)?;

    let event: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT event_type, target_type, before_json, after_json FROM audit_trail WHERE id = ?1",
    )
    .bind(&audit_id)
    .fetch_optional(store.db())
    .await
    .map_err(|e| AppError::Internal(format!("lookup audit: {e}")))?;

    let (event_type, target_type, before_json, after_json) =
        event.ok_or_else(|| AppError::NotFound(format!("audit event {audit_id} not found")))?;

    let before = before_json
        .or(after_json)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);

    let rollback_restricted = before
        .get("metadata")
        .and_then(|metadata| metadata.get("rollback_restricted"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    match (target_type.as_str(), event_type.as_str()) {
        ("provider_config", "provider.create") | ("provider_config", "provider.update") => {
            if let Some(id) = before.get("id").and_then(|v| v.as_str()) {
                sqlx::query("DELETE FROM provider_config WHERE id = ?1")
                    .bind(id)
                    .execute(store.db())
                    .await
                    .map_err(|e| AppError::Internal(format!("rollback provider: {e}")))?;
            }
        }
        ("provider_config", "provider.delete") => {
            if rollback_restricted || before.get("metadata").is_some_and(contains_redacted) {
                return Err(AppError::Config(
                    "provider rollback unavailable because audit data contains redacted credentials".into(),
                ));
            }
            if let (Some(id), Some(kind), Some(base_url), Some(pool_id)) = (
                before.get("id").and_then(|v| v.as_str()),
                before.get("kind").and_then(|v| v.as_str()),
                before.get("base_url").and_then(|v| v.as_str()),
                before.get("pool_id").and_then(|v| v.as_str()),
            ) {
                sqlx::query("INSERT OR REPLACE INTO provider_config (id, kind, base_url, pool_id, metadata) VALUES (?1, ?2, ?3, ?4, ?5)")
                    .bind(id).bind(kind).bind(base_url).bind(pool_id)
                    .bind(
                        before
                            .get("metadata")
                            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".into()))
                            .unwrap_or_else(|| "{}".into()),
                    )
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
            if let (Some(id), Some(strategy), Some(keys)) = (
                before.get("id").and_then(|v| v.as_str()),
                before.get("strategy").and_then(|v| v.as_str()),
                before.get("keys").and_then(|v| v.as_array()),
            ) {
                if !keys.is_empty() {
                    let key_hash = keys[0]
                        .get("key_hash")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    return Err(AppError::Config(format!(
                        "pool rollback cannot restore key material for {key_hash}"
                    )));
                }
                sqlx::query("INSERT OR REPLACE INTO key_pool (id, strategy) VALUES (?1, ?2)")
                    .bind(id)
                    .bind(strategy)
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
            if let (Some(model), Some(pool_id), Some(default_params)) = (
                before.get("logical_model").and_then(|v| v.as_str()),
                before.get("pool_id").and_then(|v| v.as_str()),
                before.get("default_params"),
            ) {
                sqlx::query(
                    "INSERT OR REPLACE INTO routing_config (logical_model, pool_id, default_params, enabled) VALUES (?1, ?2, ?3, 1)",
                )
                .bind(model)
                .bind(pool_id)
                .bind(serde_json::to_string(default_params).unwrap_or_else(|_| "{}".into()))
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

    store.refresh_from_db().await?;

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

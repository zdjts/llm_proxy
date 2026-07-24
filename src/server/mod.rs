//! HTTP server layer: axum router, middleware chain, and request handlers.
//!
//! # Middleware order (outer → inner)
//!
//! 1. Request ID injection  (`X-Gateway-Request-Id`)
//! 2. Client API key auth  (`Authorization: Bearer`)
//! 3. Body size limit       (`server.max_body_bytes`)
//!
//! # Handlers
//!
//! - `POST /v1/chat/completions` — transparent proxy with failover
//! - `GET /v1/models`           — model listing

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::header;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Json, routing::get, routing::post};
use dashmap::DashMap;
use futures::StreamExt;
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use tower_http::limit::RequestBodyLimitLayer;
use uuid::Uuid;

use crate::alerts::{AlertEvent, AlertSnapshot};
use crate::audit::{AuditDetail, AuditFromAuth, AuditFromRouter};
use crate::auth::{self, AuthedClient};
use crate::cache::PromptCache;
use crate::config::Config;
use crate::db;
use crate::error::AppError;
use crate::provider::ProviderResponse;
use crate::provider::inspector::StreamInspector;
use crate::router::Router as ProxyRouter;
use crate::types::{ChatCompletionRequest, Model, ModelsResponse};

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub router: Arc<ProxyRouter>,
    pub db: SqlitePool,
    pub config: Arc<Config>,
    pub cache: PromptCache,
    pub alert_tx: broadcast::Sender<AlertEvent>,
    pub error_burst_counters: Arc<DashMap<(String, String), u32>>,
    pub alert_snapshot: AlertSnapshot,
}

/// Build the axum router with all middleware and handlers.
pub fn build_router(
    state: AppState,
    auth_state: auth::AuthState,
    rate_limiter: Option<Arc<crate::ratelimit::RateLimiter>>,
) -> Router {
    let limit = state.config.server.max_body_bytes;

    let api = Router::new()
        .route("/v1/chat/completions", post(chat_completions_handler))
        .route("/v1/models", get(models_handler));

    let api = if let Some(rl) = rate_limiter {
        api.route_layer(axum::middleware::from_fn_with_state(
            rl,
            crate::ratelimit::rate_limit_middleware,
        ))
    } else {
        api
    };

    let api = api
        .layer(RequestBodyLimitLayer::new(limit))
        .route_layer(axum::middleware::from_fn_with_state(
            auth_state,
            auth::require_auth,
        ))
        .layer(axum::middleware::from_fn(request_id_middleware));

    if state.config.admin.enabled {
        let admin_allowed_ips = state.config.admin.allowed_ips.clone();
        let admin_state = state.clone();
        let admin = Router::new()
            .route("/", get(crate::dashboard::cost::cost_overview_handler))
            .route(
                "/requests",
                axum::routing::get(crate::dashboard::requests::request_list_handler),
            )
            .route(
                "/keys",
                axum::routing::get(crate::dashboard::keys::key_health_handler),
            )
            .route(
                "/traffic",
                axum::routing::get(crate::dashboard::traffic::traffic_trend_handler),
            )
            .route(
                "/alerts",
                axum::routing::get(crate::dashboard::alerts::alerts_handler),
            )
            .route(
                "/cost/drilldown",
                axum::routing::get(crate::dashboard::cost_drilldown::cost_drilldown_handler),
            )
            .route(
                "/help",
                axum::routing::get(crate::dashboard::help::help_handler),
            )
            .route_layer(axum::middleware::from_fn(move |req, next| {
                ip_guard(req, next, admin_allowed_ips.clone())
            }))
            .with_state(admin_state);

        Router::new()
            .merge(api)
            .nest("/admin", admin)
            .with_state(state)
    } else {
        api.with_state(state)
    }
}

#[derive(Clone)]
struct RequestId(String);

async fn request_id_middleware(
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let id = Uuid::new_v4().to_string();
    request.extensions_mut().insert(RequestId(id.clone()));

    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::HeaderName::from_static("x-gateway-request-id"),
        header::HeaderValue::from_str(&id).unwrap(),
    );
    Ok(response)
}

/// `POST /v1/chat/completions`
async fn chat_completions_handler(
    State(state): State<AppState>,
    axum::Extension(client): axum::Extension<AuthedClient>,
    axum::Extension(RequestId(request_id)): axum::Extension<RequestId>,
    headers: axum::http::HeaderMap,
    Json(mut req): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
    let tenant_id = client.tenant_id.clone();
    let start = SystemTime::now();
    let model = req.model.clone();

    tracing::debug!("============================================================");
    tracing::debug!(
        "▶ POST /v1/chat/completions | model={} | stream={}",
        req.model,
        req.stream.unwrap_or(false)
    );
    tracing::debug!("  headers:");
    for (name, value) in headers.iter() {
        if name.as_str().to_lowercase() == "authorization" {
            tracing::debug!("    {name}: <redacted>");
        } else {
            tracing::debug!("    {name}: {:?}", value);
        }
    }
    if let Ok(body) = serde_json::to_string_pretty(&req) {
        tracing::debug!("  request body:\n{}", body);
    }

    if !req.stream.unwrap_or(false)
        && req.temperature.unwrap_or(0.0) < 0.01
        && let Some(cached) = state.cache.get(&req)
    {
        let log = db::RequestLog {
            id: request_id.clone(),
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            client_ip: None,
            model: model.clone(),
            pool_id: "cache".into(),
            key_hash: "cache-hit".into(),
            upstream: None,
            status_code: Some(200),
            latency_ms: Some(0),
            prompt_tokens: cached.usage.as_ref().map(|u| u.prompt_tokens as i64),
            completion_tokens: cached.usage.as_ref().map(|u| u.completion_tokens as i64),
            total_tokens: cached.usage.as_ref().map(|u| u.total_tokens as i64),
            is_stream: false,
            error: None,
            audit: AuditDetail::none(),
            error_code: None,
        };
        let _ = db::log_request(&state.db, &log).await;
        return Ok((StatusCode::OK, Json(cached)).into_response());
    }

    let (provider, pool, pool_id, default_params) = state.router.resolve(&model)?;

    if let (Some(serde_json::Value::Object(pm)), serde_json::Value::Object(extra)) =
        (default_params, &mut req.extra)
    {
        for (k, v) in pm {
            extra.entry(k.clone()).or_insert(v.clone());
        }
    }

    let mut retries: Vec<String> = Vec::new();

    loop {
        let key = match state.router.pick_key(pool, pool_id) {
            Some(k) => k,
            None => {
                for key_hash in &retries {
                    tracing::error!(%pool_id, %key_hash, "all keys in pool exhausted");
                }
                let _ = state.alert_tx.send(AlertEvent::PoolExhausted {
                    ts: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64,
                    pool_id: pool_id.to_owned(),
                });
                return Err(ProxyRouter::pool_exhausted(pool_id));
            }
        };

        let key_hash = db::compute_key_hash(&key.key);

        match provider.chat(req.clone(), &key).await {
            Ok(resp) => {
                let _ = state
                    .error_burst_counters
                    .remove(&(pool_id.to_owned(), key_hash.clone()));

                let elapsed = start.elapsed().unwrap_or_default();
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;
                let latency_ms = elapsed.as_millis() as i64;

                match resp {
                    ProviderResponse::Once(ref chat_resp) => {
                        tracing::debug!(
                            "◀ {} | latency={}ms | pool={}",
                            200,
                            elapsed.as_millis(),
                            pool_id
                        );
                        if let Ok(body) = serde_json::to_string_pretty(chat_resp) {
                            tracing::debug!("  response body:\n{}", body);
                        }

                        let from_provider = provider.extract_audit(&resp);
                        let from_router = AuditFromRouter {
                            retry_count: retries.len() as i32,
                            ttft_ms: None,
                        };
                        let from_auth = AuditFromAuth {
                            tenant_id: tenant_id.clone(),
                        };
                        let audit = AuditDetail {
                            from_provider,
                            from_router,
                            from_auth,
                        };

                        let usage = chat_resp.usage.unwrap_or_default();
                        let log = db::RequestLog {
                            id: request_id.clone(),
                            ts,
                            client_ip: None,
                            model: req.model.clone(),
                            pool_id: pool_id.to_owned(),
                            key_hash,
                            upstream: Some(provider.base_url().to_owned()),
                            status_code: Some(200),
                            latency_ms: Some(latency_ms),
                            prompt_tokens: Some(usage.prompt_tokens as i64),
                            completion_tokens: Some(usage.completion_tokens as i64),
                            total_tokens: Some(usage.total_tokens as i64),
                            is_stream: false,
                            error: None,
                            audit,
                            error_code: None,
                        };
                        let _ = db::log_request(&state.db, &log).await;

                        if latency_ms > state.config.alerts.min_latency_ms as i64 {
                            let _ = state.alert_tx.send(AlertEvent::LatencySpike {
                                ts,
                                model: req.model.clone(),
                                latency_ms,
                                threshold_ms: state.config.alerts.min_latency_ms,
                            });
                        }

                        if state.config.cache_max_entries > 0 {
                            state.cache.put(&req, chat_resp);
                        }

                        return Ok((StatusCode::OK, Json(chat_resp)).into_response());
                    }
                    ProviderResponse::Stream { body } => {
                        tracing::debug!(
                            "▶ streaming start | model={} | pool={}",
                            req.model,
                            pool_id
                        );
                        let stream_response = build_stream_response(
                            StreamContext {
                                request_id: request_id.clone(),
                                ts,
                                model: req.model.clone(),
                                pool_id: pool_id.to_owned(),
                                key_hash,
                                upstream: provider.base_url().to_owned(),
                                latency_ms,
                                db_pool: state.db.clone(),
                                retry_count: retries.len() as i32,
                                tenant_id: tenant_id.clone(),
                                alert_tx: state.alert_tx.clone(),
                                min_latency_ms: state.config.alerts.min_latency_ms,
                            },
                            body,
                        );

                        return Ok(stream_response);
                    }
                }
            }
            Err(AppError::Upstream {
                status,
                retryable,
                bad_key_hint,
                msg,
            }) => {
                let burst_key = (pool_id.to_owned(), key_hash.clone());
                let burst_count = state
                    .error_burst_counters
                    .entry(burst_key)
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
                if *burst_count >= state.config.alerts.min_error_burst {
                    let _ = state.alert_tx.send(AlertEvent::UpstreamError {
                        ts: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as i64,
                        pool_id: pool_id.to_owned(),
                        key_hash: key_hash.clone(),
                        error_code: status
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                        status,
                        msg: msg.clone(),
                    });
                }
                if bad_key_hint
                    && status.is_some_and(|s| state.config.failover.bad_status_codes.contains(&s))
                {
                    state.router.mark_bad(pool_id, &key);
                    retries.push(key_hash.clone());
                }

                if !retryable {
                    return Err(AppError::Upstream {
                        status,
                        retryable: false,
                        bad_key_hint: false,
                        msg,
                    });
                }

                tracing::warn!(
                    %pool_id, %key_hash, status = ?status, retryable,
                    "upstream error, retrying next key"
                );

                continue;
            }
            Err(other) => return Err(other),
        }
    }
}

struct StreamContext {
    request_id: String,
    ts: i64,
    model: String,
    pool_id: String,
    key_hash: String,
    upstream: String,
    latency_ms: i64,
    db_pool: SqlitePool,
    retry_count: i32,
    tenant_id: String,
    alert_tx: broadcast::Sender<AlertEvent>,
    min_latency_ms: u64,
}

fn build_stream_response(
    ctx: StreamContext,
    body: futures::stream::BoxStream<'static, Result<bytes::Bytes, AppError>>,
) -> Response {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(64);

    let inspector_db = ctx.db_pool.clone();
    let inspector_id = ctx.request_id.clone();
    let inspector_model = ctx.model.clone();
    let inspector_pool = ctx.pool_id.clone();
    let inspector_kh = ctx.key_hash.clone();
    let inspector_up = ctx.upstream.clone();
    let inspector_lat = ctx.latency_ms;
    let inspector_ts = ctx.ts;
    let inspector_retries = ctx.retry_count;
    let inspector_tenant = ctx.tenant_id.clone();
    let inspector_alert_tx = ctx.alert_tx.clone();
    let inspector_min_latency_ms = ctx.min_latency_ms;
    let inspector_model_alert = ctx.model.clone();

    tokio::spawn(async move {
        let mut inspector = StreamInspector::new();
        let mut buf: Vec<u8> = Vec::new();

        while let Some(bytes) = rx.recv().await {
            buf.extend_from_slice(&bytes);
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line = &buf[..pos];
                inspector.ingest_chunk(line);
                buf.drain(..=pos);
            }
        }
        if !buf.is_empty() {
            inspector.ingest_chunk(&buf);
        }

        let ttft_ms = inspector.ttft_ms();
        let from_provider = inspector.into_audit();
        let summary = inspector.finalize();
        let usage = summary.usage.unwrap_or_default();

        tracing::debug!(
            "◀ streaming end | model={} | prompt_tokens={} | completion_tokens={} | ttft_ms={:?} | finish_reason={:?}",
            inspector_model,
            usage.prompt_tokens,
            usage.completion_tokens,
            ttft_ms,
            summary.finish_reason,
        );

        let from_router = AuditFromRouter {
            retry_count: inspector_retries,
            ttft_ms,
        };
        let from_auth = AuditFromAuth {
            tenant_id: inspector_tenant,
        };
        let audit = AuditDetail {
            from_provider,
            from_router,
            from_auth,
        };

        let log = db::RequestLog {
            id: inspector_id,
            ts: inspector_ts,
            client_ip: None,
            model: inspector_model,
            pool_id: inspector_pool,
            key_hash: inspector_kh,
            upstream: Some(inspector_up),
            status_code: Some(200),
            latency_ms: Some(inspector_lat),
            prompt_tokens: Some(usage.prompt_tokens as i64),
            completion_tokens: Some(usage.completion_tokens as i64),
            total_tokens: Some(usage.total_tokens as i64),
            is_stream: true,
            error: None,
            audit,
            error_code: None,
        };

        if let Err(e) = db::log_request(&inspector_db, &log).await {
            tracing::error!(error = %e, "failed to write stream log to db");
        }

        if inspector_lat > inspector_min_latency_ms as i64 {
            let _ = inspector_alert_tx.send(AlertEvent::LatencySpike {
                ts: inspector_ts,
                model: inspector_model_alert,
                latency_ms: inspector_lat,
                threshold_ms: inspector_min_latency_ms,
            });
        }
    });

    let client_stream = body.map(move |chunk| {
        chunk.inspect(|bytes| {
            let _ = tx.try_send(bytes.clone());
        })
    });

    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .status(StatusCode::OK)
        .body(Body::from_stream(client_stream))
        .unwrap()
}

/// `GET /v1/models`
async fn models_handler(
    State(state): State<AppState>,
    axum::Extension(_client): axum::Extension<AuthedClient>,
) -> Result<Json<ModelsResponse>, AppError> {
    let models: Vec<Model> = state
        .config
        .model_to_pool
        .keys()
        .map(|name| Model {
            id: name.clone(),
            object: "model".into(),
            created: 0,
            owned_by: "openai".into(),
        })
        .collect();

    Ok(Json(ModelsResponse {
        object: "list".into(),
        data: models,
    }))
}

/// IP guard middleware for `/admin/*` routes.
async fn ip_guard(
    request: Request<Body>,
    next: Next,
    allowed_ips: Vec<String>,
) -> Result<Response, Response> {
    let pass = request
        .headers()
        .get("x-real-ip")
        .or_else(|| request.headers().get("x-forwarded-for"))
        .and_then(|v| v.to_str().ok())
        .map(|ip| allowed_ips.iter().any(|a| a == ip))
        .unwrap_or(true); // direct connection assumed local

    if pass {
        Ok(next.run(request).await)
    } else {
        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap())
    }
}

//! Request handler — split from server/mod.rs (Module C2 — v2.0).
//!
//! Failover/retry across keys is here and in `router`, not in `Provider::chat`.
//! Server/router match only `Arc<dyn Provider>`. SSE stays in `sse_relay`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use tokio::sync::broadcast;

use crate::alerts::AlertEvent;
use crate::audit::{AuditDetail, AuditFromAuth, AuditFromRouter};
use crate::auth::AuthedClient;
use crate::db;
use crate::error::AppError;
use crate::provider::ProviderResponse;
use crate::provider::inspector::StreamInspector;
use crate::types::{ChatCompletionRequest, ModelMetadataResponse, ModelsResponse};

use super::AppState;

#[derive(Clone)]
pub struct RequestId(pub String);

pub(crate) struct StreamContext {
    pub request_id: String,
    pub ts: i64,
    pub model: String,
    pub pool_id: String,
    pub key_hash: String,
    pub upstream: String,
    pub latency_ms: i64,
    pub db_pool: sqlx::SqlitePool,
    pub retry_count: i32,
    pub tenant_id: String,
    pub alert_tx: broadcast::Sender<AlertEvent>,
    pub min_latency_ms: u64,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
    pricing: Arc<crate::config::pricing::PricingConfig>,
    pub budget_manager: Option<std::sync::Arc<crate::budget::BudgetManager>>,
}

pub async fn chat_completions_handler(
    State(state): State<AppState>,
    axum::Extension(client): axum::Extension<AuthedClient>,
    axum::Extension(RequestId(request_id)): axum::Extension<RequestId>,
    headers: axum::http::HeaderMap,
    Json(mut req): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
    state.metrics.inc_total_requests();

    let client_ip = headers
        .get("x-real-ip")
        .or_else(|| headers.get("x-forwarded-for"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_owned());

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let tenant_id = client.tenant_id.clone();
    let is_streaming = req.stream.unwrap_or(false);
    let start = SystemTime::now();
    let model = req.model.clone();

    let _concurrency_guard = match state.concurrency.acquire(&tenant_id).await {
        Some(g) => g,
        None => {
            state.metrics.inc_failed();
            let _ = state.alert_tx.send(AlertEvent::RateLimited {
                ts: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64,
                tenant_id: tenant_id.clone(),
            });
            return Err(AppError::Upstream {
                status: Some(429),
                retryable: true,
                bad_key_hint: false,
                msg: "tenant concurrency limit reached".into(),
            });
        }
    };

    // ── v4.0 Track I (AUDIT-13 Fix): budget pre-check ──
    if let Some(ref bm) = state.budget_manager {
        let pricing = state.config_store.pricing().await;
        let cost_est = estimate_cost(&pricing, &model, &tenant_id);
        let check = bm.check_budget(&tenant_id, cost_est).await?;
        if !check.allowed {
            state.metrics.inc_failed();
            return Err(AppError::Upstream {
                status: Some(429),
                retryable: false,
                bad_key_hint: false,
                msg: format!(
                    "budget exceeded: {}",
                    check.denied_by.as_deref().unwrap_or("unknown")
                ),
            });
        }
    };

    if is_streaming {
        state.metrics.inc_stream();
    }

    tracing::debug!(
        "POST /v1/chat/completions | model={} | stream={} | ip={:?} | ua={:?}",
        req.model,
        is_streaming,
        client_ip,
        user_agent,
    );

    if !is_streaming
        && req.temperature.unwrap_or(0.0) < 0.01
        && let Some(cached) = state.cache.get(&req)
    {
        state.metrics.inc_cache_hit();
        let pricing = state.config_store.pricing().await;
        let cost = compute_cost(&pricing, &model, &tenant_id, cached.usage.as_ref());
        let log = db::RequestLog {
            id: request_id.clone(),
            ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            client_ip,
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
            user_agent,
            cost_usd: cost,
        };
        let _ = db::log_request(&state.db, &log).await;
        return Ok((StatusCode::OK, Json(cached)).into_response());
    }

    let chat_service = crate::chat_service::ChatCompletionService::new(state.router.clone());
    let router = chat_service.snapshot();
    let (provider, pool, pool_id, default_params) = chat_service.resolve(&router, &model)?;

    if let (Some(serde_json::Value::Object(pm)), serde_json::Value::Object(extra)) =
        (default_params, &mut req.extra)
    {
        for (k, v) in pm {
            extra.entry(k.clone()).or_insert(v.clone());
        }
    }

    let mut retries: Vec<String> = Vec::new();
    let runtime_policy = state.config_store.runtime().await;
    let max_retries = runtime_policy.failover.max_retries as usize;
    let mut attempts: usize = 0;

    loop {
        attempts += 1;
        let key = match chat_service.pick_key(&router, pool, pool_id) {
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
                return Err(crate::router::Router::pool_exhausted(pool_id));
            }
        };

        let key_hash = db::compute_key_hash(&key.key);

        if !state.circuit_breaker.allow(pool_id, &key_hash) {
            retries.push(key_hash.clone());
            tracing::debug!(%pool_id, %key_hash, "circuit breaker open, skipping key");
            continue;
        }

        match provider.chat(req.clone(), &key).await {
            Ok(resp) => {
                state.circuit_breaker.record_success(pool_id, &key_hash);
                let _ = state
                    .error_burst_counters
                    .remove(&(pool_id.to_owned(), key_hash.clone()));

                let elapsed = start.elapsed().unwrap_or_default();
                state.metrics.record_latency_ms(elapsed.as_millis() as u64);
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;
                let latency_ms = elapsed.as_millis() as i64;

                match resp {
                    ProviderResponse::Once(ref chat_resp) => {
                        tracing::debug!(
                            "{} | latency={}ms | pool={}",
                            200,
                            elapsed.as_millis(),
                            pool_id
                        );

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
                        state.metrics.add_prompt_tokens(usage.prompt_tokens as u64);
                        state
                            .metrics
                            .add_completion_tokens(usage.completion_tokens as u64);

                        let pricing = state.config_store.pricing().await;
                        let cost = compute_cost(
                            &pricing,
                            &req.model,
                            &tenant_id,
                            chat_resp.usage.as_ref(),
                        );

                        let log = db::RequestLog {
                            id: request_id.clone(),
                            ts,
                            client_ip: client_ip.clone(),
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
                            user_agent: user_agent.clone(),
                            cost_usd: cost,
                        };
                        let _ = db::log_request(&state.db, &log).await;

                        // ── v4.0 AUDIT-13 Fix: record spend ──
                        if let (Some(bm), Some(cost)) = (&state.budget_manager, cost) {
                            let _ = bm.record_spend(&tenant_id, cost).await;
                        }

                        if latency_ms > runtime_policy.alerts.min_latency_ms as i64 {
                            let _ = state.alert_tx.send(AlertEvent::LatencySpike {
                                ts,
                                model: req.model.clone(),
                                latency_ms,
                                threshold_ms: runtime_policy.alerts.min_latency_ms,
                            });
                        }

                        if runtime_policy.cache_max_entries > 0 {
                            state.cache.put(&req, chat_resp);
                        }

                        return Ok((StatusCode::OK, Json(chat_resp)).into_response());
                    }
                    ProviderResponse::Stream { body } => {
                        tracing::debug!("streaming start | model={} | pool={}", req.model, pool_id);
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
                                min_latency_ms: runtime_policy.alerts.min_latency_ms,
                                user_agent: user_agent.clone(),
                                client_ip: client_ip.clone(),
                                pricing: state.config_store.pricing().await,
                                budget_manager: state.budget_manager.clone(),
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
                if *burst_count >= runtime_policy.alerts.min_error_burst {
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
                    && status.is_some_and(|s| runtime_policy.failover.bad_status_codes.contains(&s))
                {
                    chat_service.mark_bad(&router, pool_id, &key);
                    state.circuit_breaker.record_failure(pool_id, &key_hash);
                    state.metrics.inc_key_demotion();
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

                if attempts > max_retries {
                    tracing::warn!(
                        %pool_id, %key_hash, attempts, max_retries,
                        status = ?status,
                        error = %msg,
                        "max retries exceeded"
                    );
                    return Err(AppError::Upstream {
                        status,
                        retryable: false,
                        bad_key_hint: false,
                        msg: format!("max retries ({max_retries}) exceeded: {msg}"),
                    });
                }

                tracing::warn!(
                    %pool_id, %key_hash, status = ?status, retryable,
                    error = %msg,
                    "upstream error, retrying next key"
                );

                continue;
            }
            Err(other) => return Err(other),
        }
    }
}

pub async fn model_metadata_handler(
    State(state): State<AppState>,
    axum::Extension(_client): axum::Extension<AuthedClient>,
) -> Result<Json<ModelMetadataResponse>, AppError> {
    Ok(Json(ModelMetadataResponse {
        object: "model_metadata_list".into(),
        data: state.catalog.list_metadata().await,
    }))
}

pub async fn models_handler(
    State(state): State<AppState>,
    axum::Extension(_client): axum::Extension<AuthedClient>,
) -> Result<Json<ModelsResponse>, AppError> {
    Ok(Json(ModelsResponse {
        object: "list".into(),
        data: state.catalog.list_models(),
    }))
}

pub(crate) fn build_stream_response(
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
    let inspector_user_agent = ctx.user_agent.clone();
    let inspector_client_ip = ctx.client_ip.clone();
    let inspector_pricing = ctx.pricing.clone();
    let inspector_budget = ctx.budget_manager.clone();

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

        let cost = compute_cost(
            &inspector_pricing,
            &inspector_model,
            &inspector_tenant,
            Some(&usage),
        );

        tracing::debug!(
            "streaming end | model={} | prompt_tokens={} | completion_tokens={} | ttft_ms={:?}",
            inspector_model,
            usage.prompt_tokens,
            usage.completion_tokens,
            ttft_ms,
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
            client_ip: inspector_client_ip,
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
            user_agent: inspector_user_agent,
            cost_usd: cost,
        };

        if let Err(e) = db::log_request(&inspector_db, &log).await {
            tracing::error!(error = %e, "failed to write stream log to db");
        }

        // ── v4.0 AUDIT-13 Fix: record spend for streaming requests ──
        if let (Some(bm), Some(cost)) = (&inspector_budget, log.cost_usd) {
            let _ = bm.record_spend(&log.audit.from_auth.tenant_id, cost).await;
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
        .header(header::CACHE_CONTROL, "no-cache, no-transform")
        .header("X-Accel-Buffering", "no")
        .header("Connection", "keep-alive")
        .status(StatusCode::OK)
        .body(Body::from_stream(client_stream))
        .unwrap()
}

pub(crate) fn compute_cost(
    pricing: &crate::config::pricing::PricingConfig,
    model: &str,
    tenant: &str,
    usage: Option<&crate::types::Usage>,
) -> Option<f64> {
    let usage = usage?;
    let accounting = pricing.accounting();
    let price = accounting.lookup(model, Some(tenant));
    if price.prompt == 0.0 && price.completion == 0.0 {
        return None;
    }
    let prompt_cost = usage.prompt_tokens as f64 * price.prompt / 1_000_000.0;
    let completion_cost = usage.completion_tokens as f64 * price.completion / 1_000_000.0;
    Some(prompt_cost + completion_cost)
}

/// Conservative cost estimate for budget pre-check (before actual usage is known).
/// Uses pricing table and a default 1000-token estimate.
pub(crate) fn estimate_cost(
    pricing: &crate::config::pricing::PricingConfig,
    model: &str,
    tenant: &str,
) -> f64 {
    let accounting = pricing.accounting();
    let price = accounting.lookup(model, Some(tenant));
    let prompt_est = 1000.0 * price.prompt / 1_000_000.0;
    let completion_est = 1000.0 * price.completion / 1_000_000.0;
    prompt_est + completion_est
}

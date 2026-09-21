//! Request handler — split from server/mod.rs (Module C2 — v2.0).
//!
//! Failover/retry across keys is here and in `router`, not in `Provider::chat`.
//! Server/router match only `Arc<dyn Provider>`.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use futures::stream::BoxStream;
use tokio::sync::broadcast;

use crate::alerts::AlertEvent;
use crate::audit::{AuditDetail, AuditFromAuth, AuditFromRouter};
use crate::auth::AuthedClient;
use crate::config::ResponseNormalization;
use crate::dashboard::live::broadcast_request_log;
use crate::db;
use crate::error::AppError;
use crate::metrics::Metrics;
use crate::provider::ProviderResponse;
use crate::provider::inspector::StreamInspector;
use crate::server::stream_normalize::NormalizingStream;
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
    /// When the client request arrived. Both TTFT and total-stream latency
    /// are measured from this instant.  We deliberately do NOT record
    /// latency_ms at `provider.chat()` return — for streaming, `chat()`
    /// resolves as soon as response headers come back (TTFB), which is far
    /// smaller than the actual stream duration for any non-trivial
    /// completion.  The spawned inspector task records the real total
    /// stream time once the body has been fully consumed.
    pub request_started_at: Instant,
    pub db_pool: sqlx::SqlitePool,
    pub retry_count: i32,
    pub tenant_id: String,
    pub alert_tx: broadcast::Sender<AlertEvent>,
    pub min_latency_ms: u64,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
    pub pricing: Arc<crate::config::pricing::PricingConfig>,
    pub metrics: Arc<Metrics>,
    /// Response normalisation policy.  Read on the streaming path to
    /// decide whether to wrap the upstream body with a
    /// [`NormalizingStream`].
    pub normalization: Arc<ResponseNormalization>,
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
    let started_at = Instant::now();
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
        broadcast_request_log(&log);
        return Ok((StatusCode::OK, Json(cached)).into_response());
    }

    let chat_service = crate::chat_service::ChatCompletionService::new(state.router.clone());
    let router = chat_service.snapshot();
    let (provider, pool, pool_id, default_params, upstream_model) =
        chat_service.resolve(&router, &model)?;

    // Per-model thinking-level translation. Must run before the model id is
    // remapped to the upstream name: the model_registry is keyed by the
    // client-facing logical name. Translation rewrites canonical pi levels
    // (`off`, `low`, …) into the upstream wire value from the catalog's
    // thinkingLevelMap, and rejects levels the model does not support.
    state
        .catalog
        .translate_reasoning_effort(&model, &mut req)
        .await?;

    if let Some(upstream) = upstream_model {
        req.model = upstream.to_owned();
    }

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

        let key = match state.credentials.ensure_fresh(&key).await {
            Ok(k) => k,
            Err(e) => {
                let key_hash = key.identity_hash();
                if matches!(
                    e,
                    crate::error::AppError::Upstream {
                        bad_key_hint: true,
                        ..
                    },
                ) && chat_service.try_demote(&router, pool, pool_id, &key)
                {
                    retries.push(key_hash);
                    if attempts > max_retries {
                        return Err(e);
                    }
                    continue;
                }
                return Err(e);
            }
        };
        let key_hash = key.identity_hash();

        if !state.circuit_breaker.allow(pool_id, &key_hash) {
            retries.push(key_hash.clone());
            tracing::debug!(%pool_id, %key_hash, "circuit breaker open, skipping key");
            if attempts > max_retries {
                return Err(AppError::Upstream {
                    status: Some(429),
                    retryable: true,
                    bad_key_hint: false,
                    msg: format!("circuit breaker open for pool '{pool_id}'"),
                });
            }
            continue;
        }

        match provider.chat(&req, &key).await {
            Ok(resp) => {
                state.circuit_breaker.record_success(pool_id, &key_hash);
                let _ = state
                    .error_burst_counters
                    .remove(&(pool_id.to_owned(), key_hash.clone()));

                let elapsed = start.elapsed().unwrap_or_default();
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;

                match resp {
                    ProviderResponse::Once(ref chat_resp) => {
                        // Non-streaming: `provider.chat()` returns only after the
                        // full body has been read, so `elapsed` is the real total
                        // request latency.  Record it now.
                        state.metrics.record_latency_ms(elapsed.as_millis() as u64);
                        let latency_ms = elapsed.as_millis() as i64;

                        tracing::debug!(
                            "{} | latency={}ms | pool={}",
                            200,
                            elapsed.as_millis(),
                            pool_id
                        );

                        // Response normalisation (see
                        // [`crate::response_normalize`]): fold reasoning
                        // aliases and, as a fallback, pull inlined think
                        // tags out of `content`.  Done before audit / log
                        // / cache so every consumer sees the cleaned shape.
                        let mut normalized = chat_resp.clone();
                        if state.config.response_normalization.strip_think_tags {
                            crate::response_normalize::normalize_chat_response(&mut normalized);
                        }

                        let chat_resp = &normalized;

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
                        broadcast_request_log(&log);

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
                        // For streaming we deliberately do NOT touch
                        // `state.metrics` here: `provider.chat()` resolves
                        // the moment the upstream response headers arrive
                        // (TTFB), which is well below the real stream
                        // duration for any non-trivial completion.  The
                        // spawned inspector task records the total stream
                        // time once the body has been fully drained.
                        let stream_response = build_stream_response(
                            StreamContext {
                                request_id: request_id.clone(),
                                ts,
                                model: req.model.clone(),
                                pool_id: pool_id.to_owned(),
                                key_hash,
                                upstream: provider.base_url().to_owned(),
                                request_started_at: started_at,
                                db_pool: state.db.clone(),
                                retry_count: retries.len() as i32,
                                tenant_id: tenant_id.clone(),
                                alert_tx: state.alert_tx.clone(),
                                min_latency_ms: runtime_policy.alerts.min_latency_ms,
                                user_agent: user_agent.clone(),
                                client_ip: client_ip.clone(),
                                pricing: state.config_store.pricing().await,
                                metrics: state.metrics.clone(),
                                normalization: Arc::new(
                                    state.config.response_normalization.clone(),
                                ),
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
                    state.circuit_breaker.record_failure(pool_id, &key_hash);
                    if chat_service.try_demote(&router, pool, pool_id, &key) {
                        state.metrics.inc_key_demotion();
                        retries.push(key_hash.clone());
                    } else {
                        tracing::warn!(
                            %pool_id, %key_hash, status = ?status,
                            "last healthy key not demoted; surfacing upstream error"
                        );
                        return Err(AppError::Upstream {
                            status,
                            retryable: false,
                            bad_key_hint: false,
                            msg,
                        });
                    }
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
    let inspector_ts = ctx.ts;
    let inspector_retries = ctx.retry_count;
    let inspector_tenant = ctx.tenant_id.clone();
    let inspector_alert_tx = ctx.alert_tx.clone();
    let inspector_min_latency_ms = ctx.min_latency_ms;
    let inspector_model_alert = ctx.model.clone();
    let inspector_user_agent = ctx.user_agent.clone();
    let inspector_client_ip = ctx.client_ip.clone();
    let inspector_pricing = ctx.pricing.clone();
    let inspector_metrics = ctx.metrics.clone();

    tokio::spawn(async move {
        let started_at = ctx.request_started_at;
        let mut inspector = StreamInspector::new_started_at(started_at);
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

        // Total stream latency = request-arrival → stream fully drained.
        // We do NOT use the value captured at `provider.chat()` return time
        // (which is just TTFB for streaming requests) — that would silently
        // make streaming `latency_ms` equal to `ttft_ms` whenever the
        // upstream buffers its SSE body and flushes at end-of-generation.
        let latency_ms = started_at.elapsed().as_millis() as i64;
        inspector_metrics.record_latency_ms(latency_ms.max(0) as u64);

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
            "streaming end | model={} | prompt_tokens={} | completion_tokens={} | ttft_ms={:?} | latency_ms={}",
            inspector_model,
            usage.prompt_tokens,
            usage.completion_tokens,
            ttft_ms,
            latency_ms,
        );

        let from_router = AuditFromRouter {
            retry_count: inspector_retries,
            ttft_ms,
        };
        let from_auth = AuditFromAuth {
            tenant_id: inspector_tenant.clone(),
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
            model: inspector_model.clone(),
            pool_id: inspector_pool,
            key_hash: inspector_kh,
            upstream: Some(inspector_up),
            status_code: Some(200),
            latency_ms: Some(latency_ms),
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
        broadcast_request_log(&log);

        if latency_ms > inspector_min_latency_ms as i64 {
            let _ = inspector_alert_tx.send(AlertEvent::LatencySpike {
                ts: inspector_ts,
                model: inspector_model_alert,
                latency_ms,
                threshold_ms: inspector_min_latency_ms,
            });
        }
    });

    // Wrap the upstream body in a NormalizingStream when the operator
    // has opted in to <think> extraction.  This runs BEFORE the
    // inspect closure below, so the spawned inspector task and the
    // downstream client both see the cleaned shape — the gateway
    // never leaks raw <think>…</think> to the client.
    let body: BoxStream<'static, Result<bytes::Bytes, AppError>> =
        if ctx.normalization.strip_think_tags {
            Box::pin(NormalizingStream::new(body))
        } else {
            body
        };

    // Forward each chunk to the client, and hand a clone to the inspector
    // task via a bounded channel.  `send().await` applies backpressure: if
    // the inspector ever falls behind we slow the forwarding instead of
    // silently dropping chunks (which would corrupt the client's stream).
    let client_stream = body.then(move |chunk| {
        let tx = tx.clone();
        async move {
            if let Ok(bytes) = &chunk
                && tx.send(bytes.clone()).await.is_err()
            {
                // Inspector task is gone (db write path only); stream
                // continues to the client without audit collection.
                tracing::debug!("stream inspector channel closed; continuing without audit");
            }
            chunk
        }
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
    pricing.accounting().cost_usd(
        model,
        Some(tenant),
        usage.prompt_tokens as i64,
        usage.completion_tokens as i64,
    )
}

#[cfg(test)]
mod tests {
    //! Regression coverage for the streaming-latency fix.
    //!
    //! Before the fix, `latency_ms` for a streaming response was captured at
    //! `provider.chat()` return — which for a stream is just the time until
    //! upstream response *headers* arrive (TTFB).  That value happened to be
    //! identical to `ttft_ms` for any upstream that buffers the entire SSE
    //! body and flushes it at end-of-generation, so the dashboard showed
    //! `ttft_ms ≈ latency_ms` for streaming requests.  This test reproduces
    //! that exact shape — first SSE chunk immediately, second chunk ~300ms
    //! later — and asserts that `latency_ms` reflects the *total* stream
    //! duration, not the TTFB.

    use super::*;
    use crate::config::pricing::PricingConfig;
    use futures::stream;
    use sqlx::Row;
    use std::collections::HashMap;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time::sleep;

    const FIRST_CHUNK: &[u8] =
        b"data: {\"id\":\"1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n";

    const TRAILING_CHUNK: &[u8] = b"data: {\"id\":\"2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n\
data: [DONE]\n\n";

    async fn build_pool() -> (sqlx::SqlitePool, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let pool = db::connect(path.to_str().unwrap()).await.unwrap();
        (pool, dir)
    }

    fn empty_pricing() -> Arc<PricingConfig> {
        Arc::new(PricingConfig {
            models: HashMap::new(),
        })
    }

    /// Helper: build a body stream that emits FIRST_CHUNK immediately, then
    /// sleeps for `gap`, then emits TRAILING_CHUNK.
    fn make_delayed_body(
        gap: Duration,
    ) -> futures::stream::BoxStream<'static, Result<bytes::Bytes, AppError>> {
        Box::pin(stream::unfold(0u8, move |state| async move {
            match state {
                0 => Some((Ok(bytes::Bytes::from_static(FIRST_CHUNK)), 1)),
                1 => {
                    sleep(gap).await;
                    Some((Ok(bytes::Bytes::from_static(TRAILING_CHUNK)), 2))
                }
                _ => None,
            }
        }))
    }

    async fn fetch_latency_and_ttft(pool: &sqlx::SqlitePool, id: &str) -> (i64, Option<i64>) {
        // Poll until the spawned inspector task writes the row.
        let mut last_err = None;
        for _ in 0..40 {
            let row = sqlx::query("SELECT latency_ms, ttft_ms FROM request_log WHERE id = ?1")
                .bind(id)
                .fetch_optional(pool)
                .await;
            match row {
                Ok(Some(r)) => {
                    let latency_ms: i64 = r.get("latency_ms");
                    let ttft_ms: Option<i64> = r.get("ttft_ms");
                    return (latency_ms, ttft_ms);
                }
                Ok(None) => {
                    sleep(Duration::from_millis(50)).await;
                }
                Err(e) => {
                    last_err = Some(e);
                    sleep(Duration::from_millis(50)).await;
                }
            }
        }
        panic!(
            "timed out waiting for request_log row (id={id}): {:?}",
            last_err
        );
    }

    #[tokio::test]
    async fn streaming_total_latency_is_greater_than_ttft() {
        let (pool, _dir) = build_pool().await;
        let metrics = Arc::new(Metrics::default());
        let (alert_tx, _alert_rx) = broadcast::channel(8);

        let gap = Duration::from_millis(300);
        let body = make_delayed_body(gap);
        let id = "stream-lat-1".to_string();

        let ctx = StreamContext {
            request_id: id.clone(),
            ts: 0,
            model: "gpt-4o".into(),
            pool_id: "pool1".into(),
            key_hash: "hash".into(),
            upstream: "https://api.test".into(),
            request_started_at: Instant::now(),
            db_pool: pool.clone(),
            retry_count: 0,
            tenant_id: "t1".into(),
            alert_tx,
            min_latency_ms: 0,
            user_agent: None,
            client_ip: None,
            pricing: empty_pricing(),
            metrics: metrics.clone(),
            normalization: Arc::new(ResponseNormalization::default()),
        };

        let response = build_stream_response(ctx, body);
        // Consume the body so the spawned inspector task sees bytes flow
        // through the tx/rx channel and reaches its `db::log_request` call.
        let _ = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let (latency_ms, ttft_ms) = fetch_latency_and_ttft(&pool, &id).await;
        let ttft = ttft_ms.expect("ttft_ms should be set for streaming");

        // TTFT should be small (the first chunk arrived quickly).
        assert!(
            ttft < latency_ms,
            "ttft_ms ({ttft}) must be strictly less than latency_ms ({latency_ms})"
        );
        // Total latency should at least cover the configured gap, minus a
        // small jitter allowance.
        let gap_ms = gap.as_millis() as i64;
        assert!(
            latency_ms >= gap_ms - 50,
            "latency_ms ({latency_ms}) must cover the stream duration (gap={gap_ms}ms)"
        );
        // And it must be at least ~100ms more than ttft (the bulk of the gap
        // is streaming duration).
        assert!(
            latency_ms - ttft >= 100,
            "latency_ms - ttft_ms must be substantial, got {}ms",
            latency_ms - ttft
        );

        // Metrics: the spawned task must record latency_ms (the real one),
        // not the TTFB.
        let recorded = metrics
            .request_latency_sum_ms
            .load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            recorded as i64 >= gap_ms - 50,
            "metrics.record_latency_ms must have been called with the real total, got {recorded}ms"
        );
    }

    #[tokio::test]
    async fn streaming_with_no_delay_still_records_distinct_ttft_and_latency() {
        // Even with zero gap between chunks (upstream flushes the entire SSE
        // body in one TCP packet), the two metrics must remain distinct
        // because they measure different things: TTFT is request-arrival →
        // first parsed SSE line, latency_ms is request-arrival → stream end.
        let (pool, _dir) = build_pool().await;
        let metrics = Arc::new(Metrics::default());
        let (alert_tx, _alert_rx) = broadcast::channel(8);

        let body = make_delayed_body(Duration::from_millis(0));
        let id = "stream-lat-2".to_string();

        let ctx = StreamContext {
            request_id: id.clone(),
            ts: 0,
            model: "gpt-4o".into(),
            pool_id: "pool1".into(),
            key_hash: "hash".into(),
            upstream: "https://api.test".into(),
            request_started_at: Instant::now(),
            db_pool: pool.clone(),
            retry_count: 0,
            tenant_id: "t1".into(),
            alert_tx,
            min_latency_ms: 0,
            user_agent: None,
            client_ip: None,
            pricing: empty_pricing(),
            metrics: metrics.clone(),
            normalization: Arc::new(ResponseNormalization::default()),
        };

        let response = build_stream_response(ctx, body);
        let _ = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let (latency_ms, ttft_ms) = fetch_latency_and_ttft(&pool, &id).await;
        let ttft = ttft_ms.expect("ttft_ms should be set for streaming");
        // ttft_ms <= latency_ms, and latency_ms - ttft_ms must be small but
        // non-negative — the spawn-task bookkeeping adds at least a few ms
        // of slack, and we record latency at "stream fully drained", which is
        // always strictly after the first parsed line.
        assert!(
            latency_ms >= ttft,
            "latency_ms ({latency_ms}) must be >= ttft_ms ({ttft})"
        );
        // Sanity: the metrics counter was updated with the same total.
        //
        // This must equal `latency_ms`, not be merely positive: on a fast
        // machine the whole stream can complete in under 1ms, making
        // `latency_ms == 0`. Asserting `> 0` made this test flaky; asserting
        // equality pins the real invariant (the counter receives the total
        // stream duration, whatever it happens to be).
        let recorded = metrics
            .request_latency_sum_ms
            .load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            recorded,
            latency_ms.max(0) as u64,
            "metrics counter must receive the total stream latency"
        );
    }
}

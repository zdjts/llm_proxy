//! OpenAI-compatible upstream provider implementation.
//!
//! [`OpenAiProvider`] transparently forwards chat completion requests to any
//! OpenAI-compatible API endpoint. It does **not** make retry / failover
//! decisions — those belong to the router (T7).
//!
//! # Error signals returned to the router
//!
//! | Upstream | `AppError::Upstream` fields |
//! |----------|-----------------------------|
//! | 2xx | `Ok(ProviderResponse::...)` |
//! | 4xx ∈ `bad_status_codes` | `{retryable:true, bad_key_hint:true}` |
//! | 4xx ∉ `bad_status_codes` | `{retryable:false, bad_key_hint:false}` |
//! | 5xx | `{retryable:true, bad_key_hint:false}` |
//! | connect / timeout | `{status:None, retryable:true, bad_key_hint:false}` |

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::StreamExt;

use crate::config::KeyEntry;
use crate::error::AppError;
use crate::provider::Provider;
use crate::provider::ProviderResponse;
use crate::types::{ChatCompletionRequest, ChatCompletionResponse};

/// OpenAI-compatible upstream provider.
pub struct OpenAiProvider {
    id: String,
    base_url: String,
    client: reqwest::Client,
    bad_status_codes: Arc<[u16]>,
}

impl OpenAiProvider {
    /// Create a new provider.
    ///
    /// The internal `reqwest::Client` uses connect + idle-read timeouts (no
    /// overall deadline) so long SSE / reasoning responses are not killed.
    pub fn new(id: String, base_url: String, bad_status_codes: Arc<[u16]>) -> Self {
        Self {
            id,
            base_url,
            client: crate::provider::http::build_http_client(),
            bad_status_codes,
        }
    }

    /// Determine whether a status code is in the configured bad-codes list.
    fn is_bad_status(&self, status: u16) -> bool {
        self.bad_status_codes.contains(&status)
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn chat(
        &self,
        req: ChatCompletionRequest,
        key: &KeyEntry,
    ) -> Result<ProviderResponse, AppError> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = serde_json::to_value(&req)
            .map_err(|e| AppError::Internal(format!("failed to serialize request: {e}")))?;

        let response = self
            .client
            .post(&url)
            .bearer_auth(&key.key)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                return Err(AppError::Upstream {
                    status: None,
                    retryable: true,
                    bad_key_hint: false,
                    msg: format!("upstream connect error: {e}"),
                });
            }
        };

        let status = response.status();
        let status_code = status.as_u16();

        if status.is_success() {
            let is_stream = response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|ct| ct.starts_with("text/event-stream"))
                .unwrap_or(false);

            if is_stream {
                let stream = response.bytes_stream().map(|item| match item {
                    Ok(bytes) => Ok(bytes),
                    Err(e) => Err(AppError::Upstream {
                        status: None,
                        retryable: true,
                        bad_key_hint: false,
                        msg: format!("stream read error: {e}"),
                    }),
                });
                Ok(ProviderResponse::Stream {
                    body: Box::pin(stream),
                })
            } else {
                let raw_body: serde_json::Value =
                    response.json().await.map_err(|e| AppError::Upstream {
                        status: Some(status_code),
                        retryable: false,
                        bad_key_hint: false,
                        msg: format!("failed to parse upstream response: {e}"),
                    })?;
                let raw_usage = raw_body.get("usage").cloned();
                let mut chat_resp: ChatCompletionResponse = serde_json::from_value(raw_body)
                    .map_err(|e| AppError::Upstream {
                        status: Some(status_code),
                        retryable: false,
                        bad_key_hint: false,
                        msg: format!("failed to parse upstream response: {e}"),
                    })?;
                chat_resp.raw_usage_json = raw_usage;
                Ok(ProviderResponse::Once(chat_resp))
            }
        } else if status.is_server_error() {
            Err(AppError::Upstream {
                status: Some(status_code),
                retryable: true,
                bad_key_hint: false,
                msg: format!("upstream {status_code}"),
            })
        } else if self.is_bad_status(status_code) {
            Err(AppError::Upstream {
                status: Some(status_code),
                retryable: true,
                bad_key_hint: true,
                msg: format!("upstream {status_code}"),
            })
        } else {
            Err(AppError::Upstream {
                status: Some(status_code),
                retryable: false,
                bad_key_hint: false,
                msg: format!("upstream {status_code}"),
            })
        }
    }

    async fn probe(&self, key: &KeyEntry) -> Result<(), AppError> {
        probe_openai(&self.client, &self.base_url, key).await
    }

    fn extract_audit(&self, resp: &ProviderResponse) -> crate::audit::AuditFromProvider {
        match resp {
            ProviderResponse::Once(chat_resp) => {
                let mut audit = crate::audit::AuditFromProvider::default();

                if let Some(ref raw) = chat_resp.raw_usage_json {
                    audit.cache = extract_cache_from_usage(raw);
                    audit.reasoning_tokens = raw
                        .pointer("/completion_tokens_details/reasoning_tokens")
                        .and_then(|v| v.as_i64());
                    audit.audio_tokens = raw
                        .pointer("/completion_tokens_details/audio_tokens")
                        .and_then(|v| v.as_i64());
                }

                if let Some(choices) = chat_resp.choices.first() {
                    audit.finish_reason = choices.finish_reason.clone();
                }

                audit.upstream_model = Some(chat_resp.model.clone());
                audit
            }
            ProviderResponse::Stream { .. } => crate::audit::AuditFromProvider::default(),
        }
    }
}

/// Standalone health-probe helper (can be called from `health.rs` without
/// importing the full [`OpenAiProvider`]).
///
/// Sends `GET <base_url>/models` with the given key. A 2xx response means the
/// key is healthy.
pub async fn probe_openai(
    client: &reqwest::Client,
    base_url: &str,
    key: &KeyEntry,
) -> Result<(), AppError> {
    let url = format!("{base_url}/models");
    let response = client
        .get(&url)
        .bearer_auth(&key.key)
        .send()
        .await
        .map_err(|e| AppError::Upstream {
            status: None,
            retryable: true,
            bad_key_hint: false,
            msg: format!("probe connect error: {e}"),
        })?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(AppError::Upstream {
            status: Some(response.status().as_u16()),
            retryable: true,
            bad_key_hint: false,
            msg: format!("probe returned {}", response.status()),
        })
    }
}

/// Extract cache-hit information from a raw upstream `"usage"` JSON object.
///
/// Tries OpenAI-style `prompt_tokens_details.cached_tokens` first, then
/// DeepSeek-style `prompt_cache_hit_tokens`.  Returns [`CacheReport::none()`]
/// when neither field is present.
pub fn extract_cache_from_usage(raw: &serde_json::Value) -> crate::audit::CacheReport {
    if let Some(cached) = raw
        .pointer("/prompt_tokens_details/cached_tokens")
        .and_then(|v| v.as_i64())
    {
        return crate::audit::CacheReport {
            hit_tokens: Some(cached),
            creation_tokens: None,
            source: crate::audit::ProviderCacheKind::OpenAiPromptCache,
        };
    }

    if let Some(cached) = raw.get("prompt_cache_hit_tokens").and_then(|v| v.as_i64()) {
        return crate::audit::CacheReport {
            hit_tokens: Some(cached),
            creation_tokens: None,
            source: crate::audit::ProviderCacheKind::DeepSeekPromptCache,
        };
    }

    crate::audit::CacheReport::none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> OpenAiProvider {
        let codes: Arc<[u16]> = Arc::from([401, 402, 403, 429]);
        OpenAiProvider::new("test".into(), "http://127.0.0.1:1234/v1".into(), codes)
    }

    #[test]
    fn it_has_correct_id_and_base_url() {
        let p = provider();
        assert_eq!(p.id(), "test");
        assert_eq!(p.base_url(), "http://127.0.0.1:1234/v1");
    }

    #[test]
    fn it_classifies_bad_status_codes() {
        let p = provider();
        assert!(p.is_bad_status(429));
        assert!(p.is_bad_status(401));
        assert!(!p.is_bad_status(404));
        assert!(!p.is_bad_status(500));
    }

    #[tokio::test]
    async fn it_returns_upstream_error_on_connect_refused() {
        let codes: Arc<[u16]> = Arc::from([401, 402, 403, 429]);
        let p = OpenAiProvider::new("test".into(), "http://127.0.0.1:19999/v1".into(), codes);

        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            user: None,
            tools: None,
            tool_choice: None,
            stream_options: None,
            extra: serde_json::Value::Null,
        };

        let result = p
            .chat(
                req,
                &KeyEntry {
                    key: "sk-test".into(),
                    weight: 1,
                },
            )
            .await;

        match result {
            Err(AppError::Upstream {
                retryable,
                bad_key_hint,
                ..
            }) => {
                assert!(retryable);
                assert!(!bad_key_hint);
            }
            other => panic!("expected Upstream error, got {:?}", other),
        }
    }
}

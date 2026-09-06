//! Azure OpenAI provider (T72).
//!
//! Uses Azure's API version query parameter pattern:
//! `https://{resource}.openai.azure.com/openai/deployments/{deployment}/chat/completions?api-version={version}`

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::StreamExt;

use crate::config::KeyEntry;
use crate::error::AppError;
use crate::provider::{Provider, ProviderCapabilities, ProviderResponse};
use crate::types::ChatCompletionRequest;

pub struct AzureProvider {
    id: String,
    base_url: String,
    api_version: String,
    bad_status_codes: Arc<[u16]>,
    client: reqwest::Client,
}

impl AzureProvider {
    pub fn new(
        id: String,
        base_url: String,
        api_version: String,
        bad_status_codes: Arc<[u16]>,
    ) -> Self {
        Self {
            id,
            base_url,
            api_version,
            bad_status_codes,
            client: crate::provider::http::build_http_client(),
        }
    }
}

#[async_trait]
impl Provider for AzureProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_vision: true,
            supports_tool_calling: true,
            supports_json_mode: true,
            max_context_tokens: 128_000,
            max_output_tokens: 16_384,
        }
    }

    async fn chat(
        &self,
        req: ChatCompletionRequest,
        key: &KeyEntry,
    ) -> Result<ProviderResponse, AppError> {
        let url = format!(
            "{}/chat/completions?api-version={}",
            self.base_url.trim_end_matches('/'),
            self.api_version
        );

        let is_stream = req.stream.unwrap_or(false);
        let response = self
            .client
            .post(&url)
            .header("api-key", &key.key)
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(
                    provider = %self.id,
                    url = %url,
                    error = %e,
                    "Azure upstream request failed"
                );
                AppError::Upstream {
                    status: None,
                    retryable: true,
                    bad_key_hint: false,
                    msg: format!("Azure request failed: {e}"),
                }
            })?;

        let status = response.status();
        if self.bad_status_codes.contains(&status.as_u16()) {
            let body_text = response.text().await.unwrap_or_default();
            return Err(AppError::Upstream {
                status: Some(status.as_u16()),
                retryable: status.as_u16() == 429,
                bad_key_hint: true,
                msg: format!("Azure returned {status}: {body_text}"),
            });
        }

        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(AppError::Upstream {
                status: Some(status.as_u16()),
                retryable: status.is_server_error(),
                bad_key_hint: false,
                msg: format!("Azure error {status}: {body_text}"),
            });
        }

        if is_stream {
            let stream = response.bytes_stream().map(|item| match item {
                Ok(bytes) => Ok(bytes),
                Err(e) => Err(AppError::Upstream {
                    status: None,
                    retryable: true,
                    bad_key_hint: false,
                    msg: format!("Azure stream error: {e}"),
                }),
            });
            Ok(ProviderResponse::Stream {
                body: stream.boxed(),
            })
        } else {
            let body: crate::types::ChatCompletionResponse =
                response.json().await.map_err(|e| AppError::Upstream {
                    status: Some(status.as_u16()),
                    retryable: false,
                    bad_key_hint: false,
                    msg: format!("Azure response parse error: {e}"),
                })?;
            Ok(ProviderResponse::Once(body))
        }
    }

    async fn probe(&self, key: &KeyEntry) -> Result<(), AppError> {
        let url = format!(
            "{}/chat/completions?api-version={}",
            self.base_url.trim_end_matches('/'),
            self.api_version
        );
        let probe_req = ChatCompletionRequest {
            model: "probe".into(),
            messages: vec![crate::types::Message {
                role: "user".into(),
                content: serde_json::Value::String("ping".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: Some(false),
            max_tokens: Some(1),
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

        let response = self
            .client
            .post(&url)
            .header("api-key", &key.key)
            .header("Content-Type", "application/json")
            .json(&probe_req)
            .send()
            .await
            .map_err(|e| AppError::Upstream {
                status: None,
                retryable: true,
                bad_key_hint: true,
                msg: format!("Azure probe failed: {e}"),
            })?;

        if response.status().is_success() {
            Ok(())
        } else if response.status().as_u16() == 401 || response.status().as_u16() == 403 {
            Err(AppError::Upstream {
                status: Some(response.status().as_u16()),
                retryable: false,
                bad_key_hint: true,
                msg: "Azure probe: authentication failed".into(),
            })
        } else {
            Err(AppError::Upstream {
                status: Some(response.status().as_u16()),
                retryable: true,
                bad_key_hint: false,
                msg: "Azure probe: upstream error".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_has_correct_provider_id() {
        let provider = AzureProvider::new(
            "azure-test".into(),
            "https://test.openai.azure.com/".into(),
            "2024-06-01".into(),
            Arc::from([401, 403, 429]),
        );
        assert_eq!(provider.id(), "azure-test");
        assert!(provider.base_url().contains("openai.azure.com"));
    }

    #[test]
    fn it_declares_vision_and_tool_calling() {
        let provider = AzureProvider::new(
            "azure".into(),
            "https://x.openai.azure.com/".into(),
            "2024-06-01".into(),
            Arc::from([]),
        );
        let caps = provider.capabilities();
        assert!(caps.supports_vision);
        assert!(caps.supports_tool_calling);
        assert!(caps.supports_json_mode);
        assert!(caps.max_context_tokens > 0);
    }
}

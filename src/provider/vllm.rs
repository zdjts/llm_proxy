//! vLLM (local LLM inference engine) provider (T72).
//!
//! vLLM provides an OpenAI-compatible endpoint. Model is specified via the `model`
//! field in the request body; vLLM routes to the loaded model.

use crate::config::KeyEntry;
use crate::error::AppError;
use crate::provider::{Provider, ProviderCapabilities, ProviderResponse};
use crate::types::ChatCompletionRequest;
use async_trait::async_trait;
use reqwest::Client;
use std::sync::Arc;

pub struct VllmProvider {
    id: String,
    base_url: String,
    bad_status_codes: Arc<[u16]>,
    client: Client,
}

impl VllmProvider {
    pub fn new(id: String, base_url: String, bad_status_codes: Arc<[u16]>) -> Self {
        Self {
            id,
            base_url,
            bad_status_codes,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Provider for VllmProvider {
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
            max_context_tokens: 131_072,
            max_output_tokens: 8_192,
        }
    }

    async fn chat(
        &self,
        req: ChatCompletionRequest,
        key: &KeyEntry,
    ) -> Result<ProviderResponse, AppError> {
        let is_stream = req.stream.unwrap_or(false);
        let mut builder = self.client.post(&self.base_url).json(&req);
        if !key.key.is_empty() {
            builder = builder.header("Authorization", format!("Bearer {}", key.key));
        }
        let response = builder.send().await.map_err(|e| AppError::Upstream {
            status: None,
            retryable: true,
            bad_key_hint: false,
            msg: format!("vLLM: {e}"),
        })?;

        let status = response.status();
        if self.bad_status_codes.contains(&status.as_u16()) {
            return Err(AppError::Upstream {
                status: Some(status.as_u16()),
                retryable: false,
                bad_key_hint: true,
                msg: "vLLM key rejected".into(),
            });
        }
        if !status.is_success() {
            return Err(AppError::Upstream {
                status: Some(status.as_u16()),
                retryable: status.is_server_error(),
                bad_key_hint: false,
                msg: "vLLM error".into(),
            });
        }

        if is_stream {
            use futures::stream::StreamExt;
            let stream = response.bytes_stream().map(|item| match item {
                Ok(b) => Ok(b),
                Err(e) => Err(AppError::Upstream {
                    status: None,
                    retryable: true,
                    bad_key_hint: false,
                    msg: format!("vLLM stream: {e}"),
                }),
            });
            Ok(ProviderResponse::Stream {
                body: stream.boxed(),
            })
        } else {
            let body = response.json().await.map_err(|e| AppError::Upstream {
                status: Some(status.as_u16()),
                retryable: false,
                bad_key_hint: false,
                msg: format!("vLLM parse: {e}"),
            })?;
            Ok(ProviderResponse::Once(body))
        }
    }

    async fn probe(&self, key: &KeyEntry) -> Result<(), AppError> {
        let mut builder = self.client.get(&self.base_url);
        if !key.key.is_empty() {
            builder = builder.header("Authorization", format!("Bearer {}", key.key));
        }
        builder.send().await.map_err(|e| AppError::Upstream {
            status: None,
            retryable: true,
            bad_key_hint: false,
            msg: format!("vLLM probe: {e}"),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_has_capabilities() {
        let p = VllmProvider::new(
            "vllm".into(),
            "http://localhost:8000/v1/chat/completions".into(),
            Arc::from([]),
        );
        assert!(p.capabilities().max_context_tokens > 0);
    }
}

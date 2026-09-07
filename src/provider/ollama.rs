//! Ollama (local LLM runner) provider (T72).
//!
//! Ollama provides an OpenAI-compatible endpoint via its `/v1/chat/completions` API.

use crate::config::KeyEntry;
use crate::error::AppError;
use crate::provider::{Provider, ProviderCapabilities, ProviderResponse};
use crate::types::ChatCompletionRequest;
use async_trait::async_trait;
use std::sync::Arc;

pub struct OllamaProvider {
    id: String,
    base_url: String,
    bad_status_codes: Arc<[u16]>,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(id: String, base_url: String, bad_status_codes: Arc<[u16]>) -> Self {
        Self {
            id,
            base_url,
            bad_status_codes,
            client: crate::provider::http::build_http_client(),
        }
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_vision: true, // supports llava and similar
            supports_tool_calling: true,
            supports_json_mode: true,
            max_context_tokens: 32_768,
            max_output_tokens: 4_096,
        }
    }

    async fn chat(
        &self,
        req: ChatCompletionRequest,
        key: &KeyEntry,
    ) -> Result<ProviderResponse, AppError> {
        // Ollama API key is optional (can be "ollama" for non-auth setups)
        let is_stream = req.stream.unwrap_or(false);
        let mut builder = self.client.post(&self.base_url).json(&req);
        if !key.key.is_empty() && key.key != "ollama" {
            builder = builder.header("Authorization", format!("Bearer {}", key.key));
        }
        let response = builder.send().await.map_err(|e| AppError::Upstream {
            status: None,
            retryable: true,
            bad_key_hint: false,
            msg: format!("Ollama: {e}"),
        })?;

        let status = response.status();
        if self.bad_status_codes.contains(&status.as_u16()) {
            return Err(AppError::Upstream {
                status: Some(status.as_u16()),
                retryable: false,
                bad_key_hint: true,
                msg: crate::provider::http::upstream_error_message(status.as_u16(), response).await,
            });
        }
        if !status.is_success() {
            return Err(AppError::Upstream {
                status: Some(status.as_u16()),
                retryable: status.is_server_error(),
                bad_key_hint: false,
                msg: crate::provider::http::upstream_error_message(status.as_u16(), response).await,
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
                    msg: format!("Ollama stream: {e}"),
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
                msg: format!("Ollama parse: {e}"),
            })?;
            Ok(ProviderResponse::Once(body))
        }
    }

    async fn probe(&self, key: &KeyEntry) -> Result<(), AppError> {
        let mut builder = self.client.get(&self.base_url);
        if !key.key.is_empty() && key.key != "ollama" {
            builder = builder.header("Authorization", format!("Bearer {}", key.key));
        }
        builder.send().await.map_err(|e| AppError::Upstream {
            status: None,
            retryable: true,
            bad_key_hint: false,
            msg: format!("Ollama probe: {e}"),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_has_capabilities() {
        let p = OllamaProvider::new(
            "ollama".into(),
            "http://localhost:11434/v1/chat/completions".into(),
            Arc::from([]),
        );
        assert!(p.capabilities().supports_tool_calling);
    }
}

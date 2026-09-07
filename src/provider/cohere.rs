//! Cohere provider (T72).

use crate::config::KeyEntry;
use crate::error::AppError;
use crate::provider::{Provider, ProviderCapabilities, ProviderResponse};
use crate::types::ChatCompletionRequest;
use async_trait::async_trait;
use std::sync::Arc;

pub struct CohereProvider {
    id: String,
    base_url: String,
    bad_status_codes: Arc<[u16]>,
    client: reqwest::Client,
}

impl CohereProvider {
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
impl Provider for CohereProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_vision: false,
            supports_tool_calling: true,
            supports_json_mode: true,
            max_context_tokens: 128_000,
            max_output_tokens: 8_192,
        }
    }

    async fn chat(
        &self,
        req: ChatCompletionRequest,
        key: &KeyEntry,
    ) -> Result<ProviderResponse, AppError> {
        let is_stream = req.stream.unwrap_or(false);
        let response = self
            .client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", key.key))
            .json(&req)
            .send()
            .await
            .map_err(|e| AppError::Upstream {
                status: None,
                retryable: true,
                bad_key_hint: false,
                msg: format!("Cohere: {e}"),
            })?;

        let status = response.status();
        if self.bad_status_codes.contains(&status.as_u16()) {
            return Err(AppError::Upstream {
                status: Some(status.as_u16()),
                retryable: status.as_u16() == 429,
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
                    msg: format!("Cohere stream: {e}"),
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
                msg: format!("Cohere parse: {e}"),
            })?;
            Ok(ProviderResponse::Once(body))
        }
    }

    async fn probe(&self, key: &KeyEntry) -> Result<(), AppError> {
        self.client
            .get(&self.base_url)
            .header("Authorization", format!("Bearer {}", key.key))
            .send()
            .await
            .map_err(|e| AppError::Upstream {
                status: None,
                retryable: true,
                bad_key_hint: true,
                msg: format!("Cohere probe: {e}"),
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_has_capabilities() {
        let p = CohereProvider::new(
            "cohere".into(),
            "https://api.cohere.com/v2/chat".into(),
            Arc::from([]),
        );
        assert!(p.capabilities().max_context_tokens > 0);
        assert!(!p.capabilities().supports_vision);
    }
}

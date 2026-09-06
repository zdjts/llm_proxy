//! AWS Bedrock provider — Claude and Titan models (T72).
//!
//! # IMPORTANT: SigV4 Sidecar Required
//!
//! The real AWS Bedrock API requires AWS Signature V4 authentication —
//! it does NOT accept a plain API key in a header. This provider expects
//! the `base_url` to point to a **local sidecar proxy** that performs
//! SigV4 signing before forwarding to the actual Bedrock endpoint.
//!
//! Without such a sidecar, this provider will receive 403 errors from AWS.
//! Example sidecars: aws-sigv4-proxy, envoy-aws-sigv4, or a bespoke nginx/lua
//! script. See `docs/design-provider.md` for sidecar setup instructions.
//!
//! This design decision avoids pulling in the heavy `aws-sigv4` / `aws-sdk`
//! crate chain, which would add ~30 dependencies and ~2MB to the binary
//! (violating the project's "minimal dependencies" principle, ADR-002).

use std::sync::Arc;

use async_trait::async_trait;

use crate::config::KeyEntry;
use crate::error::AppError;
use crate::provider::{Provider, ProviderCapabilities, ProviderResponse};
use crate::types::ChatCompletionRequest;

/// Bedrock provider using the Converse API via REST.
pub struct BedrockProvider {
    id: String,
    base_url: String,
    region: String,
    bad_status_codes: Arc<[u16]>,
    client: reqwest::Client,
}

impl BedrockProvider {
    pub fn new(id: String, base_url: String, region: String, bad_status_codes: Arc<[u16]>) -> Self {
        Self {
            id,
            base_url,
            region,
            bad_status_codes,
            client: crate::provider::http::build_http_client(),
        }
    }
}

#[async_trait]
impl Provider for BedrockProvider {
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
            supports_json_mode: false,
            max_context_tokens: 200_000,
            max_output_tokens: 8_192,
        }
    }

    async fn chat(
        &self,
        req: ChatCompletionRequest,
        key: &KeyEntry,
    ) -> Result<ProviderResponse, AppError> {
        // AWS Bedrock uses SigV4 signing. The key is an AWS access key.
        // For now, proxy to a local sidecar that handles signing,
        // or the base_url points to a Bedrock-compatible gateway.
        let is_stream = req.stream.unwrap_or(false);
        let response = self
            .client
            .post(&self.base_url)
            .header("x-api-key", &key.key)
            .header("x-aws-region", &self.region)
            .json(&req)
            .send()
            .await
            .map_err(|e| AppError::Upstream {
                status: None,
                retryable: true,
                bad_key_hint: false,
                msg: format!("Bedrock request failed: {e}"),
            })?;

        let status = response.status();
        if self.bad_status_codes.contains(&status.as_u16()) {
            let body_text = response.text().await.unwrap_or_default();
            return Err(AppError::Upstream {
                status: Some(status.as_u16()),
                retryable: status.as_u16() == 429,
                bad_key_hint: true,
                msg: format!("Bedrock returned {status}: {body_text}"),
            });
        }

        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(AppError::Upstream {
                status: Some(status.as_u16()),
                retryable: status.is_server_error(),
                bad_key_hint: false,
                msg: format!("Bedrock error: {body_text}"),
            });
        }

        if is_stream {
            use futures::stream::StreamExt;
            let stream = response.bytes_stream().map(|item| match item {
                Ok(bytes) => Ok(bytes),
                Err(e) => Err(AppError::Upstream {
                    status: None,
                    retryable: true,
                    bad_key_hint: false,
                    msg: format!("Bedrock stream error: {e}"),
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
                    msg: format!("Bedrock parse error: {e}"),
                })?;
            Ok(ProviderResponse::Once(body))
        }
    }

    async fn probe(&self, key: &KeyEntry) -> Result<(), AppError> {
        let response = self
            .client
            .get(&self.base_url)
            .header("x-api-key", &key.key)
            .header("x-aws-region", &self.region)
            .send()
            .await
            .map_err(|e| AppError::Upstream {
                status: None,
                retryable: true,
                bad_key_hint: true,
                msg: format!("Bedrock probe failed: {e}"),
            })?;

        if response.status().is_success() || response.status().as_u16() == 404 {
            Ok(())
        } else {
            Err(AppError::Upstream {
                status: Some(response.status().as_u16()),
                retryable: response.status().as_u16() == 429,
                bad_key_hint: response.status().as_u16() == 401
                    || response.status().as_u16() == 403,
                msg: "Bedrock probe: upstream error".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_declares_capabilities() {
        let p = BedrockProvider::new(
            "bedrock".into(),
            "https://bedrock-runtime.us-east-1.amazonaws.com".into(),
            "us-east-1".into(),
            Arc::from([]),
        );
        assert!(p.capabilities().supports_vision);
        assert!(p.capabilities().supports_tool_calling);
    }
}

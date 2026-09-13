//! Upstream provider abstraction via the [`Provider`] trait.
//!
//! MVP only implements OpenAI-compatible backends, but the trait accepts a
//! [`ChatCompletionRequest`] directly — future Anthropic / Gemini providers can
//! perform bidirectional format conversion inside their implementation.
//!
//! # Design constraints
//!
//! - The router layer only sees `Arc<dyn Provider>`. It must never downcast or
//!   `match` on concrete provider types.
//! - `Provider::chat` makes exactly one request, no built-in retry. Retry /
//!   failover logic lives in the router (T7).
//! - Per-chunk stream errors propagate as `Result<Bytes, AppError>` so the
//!   server layer can distinguish between a connection error and a valid SSE
//!   chunk.

pub mod anthropic;
pub mod anthropic_stream;
pub mod azure;
pub mod bedrock;
pub mod cohere;
pub mod gemini;
pub mod gemini_stream;
pub mod http;
pub mod inspector;
pub mod mistral;
pub mod ollama;
pub mod openai;
pub mod registry;
pub mod vllm;

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;

use crate::config::KeyEntry;
use crate::error::AppError;
use crate::types::ChatCompletionRequest;
use crate::types::ChatCompletionResponse;

/// Declared capabilities of a provider/model combination (T73).
///
/// Used by the router to validate model requests and by the frontend
/// model registry to display capability icons.
#[derive(Debug, Clone, Default)]
pub struct ProviderCapabilities {
    /// Whether this provider/model supports image/vision inputs.
    pub supports_vision: bool,
    /// Whether this provider/model supports tool/function calling.
    pub supports_tool_calling: bool,
    /// Whether this provider/model supports JSON mode (structured output).
    pub supports_json_mode: bool,
    /// Maximum context size in tokens.
    pub max_context_tokens: u32,
    /// Maximum output tokens.
    pub max_output_tokens: u32,
}

/// The result of a provider `chat()` call.
///
/// Non-streaming responses carry a fully-hydrated [`ChatCompletionResponse`];
/// streaming responses carry a per-chunk `Result<Bytes, AppError>` byte stream
/// that is relayed verbatim to the client (parsed only for usage tracking in
/// the server layer, never re-serialized).
pub enum ProviderResponse {
    Once(ChatCompletionResponse),
    Stream {
        body: BoxStream<'static, Result<Bytes, AppError>>,
    },
}

impl std::fmt::Debug for ProviderResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Once(resp) => f.debug_tuple("Once").field(resp).finish(),
            Self::Stream { .. } => f.debug_tuple("Stream").field(&"<stream>").finish(),
        }
    }
}

/// Common interface for every upstream provider.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Unique provider identifier, e.g. `"openai"`, `"deepseek"`.
    fn id(&self) -> &str;

    /// Base URL for debugging and logging.
    fn base_url(&self) -> &str;

    /// Send a chat completion request to the upstream identified by `key`.
    ///
    /// Returns [`ProviderResponse::Once`] for non-streaming or
    /// [`ProviderResponse::Stream`] for streaming responses. Per-chunk errors
    /// are surfaced via `Err(AppError)` inside the stream.
    async fn chat(
        &self,
        req: ChatCompletionRequest,
        key: &KeyEntry,
    ) -> Result<ProviderResponse, AppError>;

    /// Lightweight health probe. Called by `health.rs` to determine whether a
    /// previously-bad key has recovered.
    ///
    /// Default implementation returns an `Internal` error rather than panicking
    /// (NEW-AUDIT-12). Concrete providers MUST override this.
    async fn probe(&self, _key: &KeyEntry) -> Result<(), AppError> {
        Err(AppError::Internal(format!(
            "probe not implemented for provider {}",
            self.id()
        )))
    }

    /// Read-only sidecar: extract provider-specific audit fields from a
    /// completed response (ADR-005 v2).  Never called from the hot chat
    /// path; invoked once per request in the server handler, immediately
    /// before writing the `request_log` row.
    ///
    /// Returns [`AuditFromProvider::default()`](crate::audit::AuditFromProvider::default)
    /// when the provider has nothing to contribute.
    fn extract_audit(&self, _resp: &ProviderResponse) -> crate::audit::AuditFromProvider {
        crate::audit::AuditFromProvider::default()
    }

    /// Declared capabilities for this provider (T73).
    ///
    /// Used by the router for capability-based model validation (e.g.,
    /// rejecting a vision request on a text-only model). Default
    /// implementation returns conservative defaults.
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct MockProvider {
        response: Arc<std::sync::Mutex<Option<Result<ProviderResponse, AppError>>>>,
    }

    impl MockProvider {
        fn new(result: Result<ProviderResponse, AppError>) -> Self {
            Self {
                response: Arc::new(std::sync::Mutex::new(Some(result))),
            }
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }

        fn base_url(&self) -> &str {
            "https://mock.local/v1"
        }

        async fn chat(
            &self,
            _req: ChatCompletionRequest,
            _key: &KeyEntry,
        ) -> Result<ProviderResponse, AppError> {
            self.response.lock().unwrap().take().unwrap()
        }
    }

    #[test]
    fn it_returns_provider_id() {
        let provider = MockProvider::new(Ok(ProviderResponse::Once(ChatCompletionResponse {
            id: "test".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "mock".into(),
            choices: vec![],
            usage: None,
            raw_usage_json: None,
        })));
        assert_eq!(provider.id(), "mock");
    }

    #[test]
    fn it_returns_base_url() {
        let provider = MockProvider::new(Ok(ProviderResponse::Once(ChatCompletionResponse {
            id: "test".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "mock".into(),
            choices: vec![],
            usage: None,
            raw_usage_json: None,
        })));
        assert_eq!(provider.base_url(), "https://mock.local/v1");
    }

    #[tokio::test]
    async fn it_invokes_chat_via_trait() {
        let provider = MockProvider::new(Ok(ProviderResponse::Once(ChatCompletionResponse {
            id: "t1".into(),
            object: "chat.completion".into(),
            created: 1,
            model: "mock".into(),
            choices: vec![],
            usage: None,
            raw_usage_json: None,
        })));

        let key = KeyEntry::api_key("sk-test", 1);

        let req = ChatCompletionRequest {
            model: "mock-model".into(),
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

        let result = provider.chat(req, &key).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn it_propagates_provider_error() {
        let provider = MockProvider::new(Err(AppError::Upstream {
            status: Some(502),
            retryable: true,
            bad_key_hint: false,
            msg: "upstream down".into(),
        }));

        let result = provider
            .chat(
                ChatCompletionRequest {
                    model: "x".into(),
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
                },
                &KeyEntry::api_key("k", 1),
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::Upstream { msg, .. } => assert_eq!(msg, "upstream down"),
            _ => panic!("expected Upstream error"),
        }
    }
}

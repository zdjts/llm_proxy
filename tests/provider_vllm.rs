//! Integration tests for [`VllmProvider`] using mockito (AUDIT-06).
//!
//! Pattern identical to `tests/provider_azure.rs`.

use std::sync::Arc;

use futures::StreamExt;
use llm_proxy::config::KeyEntry;
use llm_proxy::error::AppError;
use llm_proxy::provider::Provider;
use llm_proxy::provider::ProviderResponse;
use llm_proxy::provider::vllm::VllmProvider;
use mockito::ServerGuard;

fn test_provider(server: &ServerGuard) -> VllmProvider {
    let codes: Arc<[u16]> = Arc::from([401, 402, 403, 429]);
    VllmProvider::new("vllm-test".into(), server.url(), codes)
}

fn test_key() -> KeyEntry {
    KeyEntry::api_key("sk-vllm-test", 1)
}

fn test_request() -> llm_proxy::types::ChatCompletionRequest {
    llm_proxy::types::ChatCompletionRequest {
        model: "qwen2.5-72b".into(),
        messages: serde_json::from_str(r#"[{"role":"user","content":"hi"}]"#).unwrap(),
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
    }
}

#[tokio::test]
async fn it_returns_once_on_2xx_non_stream() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/")
        .match_header("authorization", "Bearer sk-vllm-test")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"chat-1","object":"chat.completion","created":123,"model":"qwen2.5-72b","choices":[{"index":0,"message":{"role":"assistant","content":"Hello!"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#
        )
        .create_async()
        .await;

    let provider = test_provider(&server);
    let result = provider.chat(test_request(), &test_key()).await.unwrap();

    match result {
        ProviderResponse::Once(resp) => {
            assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
            assert_eq!(resp.usage.unwrap().total_tokens, 15);
        }
        _ => panic!("expected Once, got Stream"),
    }
}

#[tokio::test]
async fn it_returns_stream_on_2xx_stream() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/")
        .match_header("authorization", "Bearer sk-vllm-test")
        .with_status(200)
        .with_body(
            "data: {\"id\":\"s1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"qwen2.5-72b\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n\
             data: [DONE]\n\n",
        )
        .create_async()
        .await;

    let provider = test_provider(&server);
    let mut req = test_request();
    req.stream = Some(true);
    let result = provider.chat(req, &test_key()).await.unwrap();

    match result {
        ProviderResponse::Stream { body } => {
            let chunks: Vec<_> = body.collect().await;
            assert!(!chunks.is_empty());
        }
        _ => panic!("expected Stream, got Once"),
    }
}

#[tokio::test]
async fn it_returns_bad_key_hint_on_401() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/")
        .with_status(401)
        .with_body(r#"{"error":{"message":"Access denied"}}"#)
        .create_async()
        .await;

    let provider = test_provider(&server);
    let err = provider
        .chat(test_request(), &test_key())
        .await
        .unwrap_err();

    match err {
        AppError::Upstream {
            status,
            bad_key_hint,
            ..
        } => {
            assert_eq!(status, Some(401));
            assert!(bad_key_hint);
        }
        other => panic!("expected Upstream error, got {:?}", other),
    }
}

#[tokio::test]
async fn it_returns_retryable_true_on_503() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/")
        .with_status(503)
        .with_body("Service Unavailable")
        .create_async()
        .await;

    let provider = test_provider(&server);
    let err = provider
        .chat(test_request(), &test_key())
        .await
        .unwrap_err();

    match err {
        AppError::Upstream {
            status, retryable, ..
        } => {
            assert_eq!(status, Some(503));
            assert!(retryable);
        }
        other => panic!("expected Upstream error, got {:?}", other),
    }
}

#[tokio::test]
async fn it_handles_empty_key() {
    // vLLM allows empty key for local setups
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"chat-1","object":"chat.completion","created":123,"model":"qwen2.5-72b","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#
        )
        .create_async()
        .await;

    let codes: Arc<[u16]> = Arc::from([401, 402, 403, 429]);
    let provider = VllmProvider::new("vllm-test".into(), server.url(), codes);
    let result = provider
        .chat(test_request(), &KeyEntry::api_key(String::new(), 1))
        .await;
    assert!(result.is_ok());
}

#[test]
fn it_declares_capabilities() {
    let provider = VllmProvider::new("test".into(), "http://localhost:1/".into(), Arc::from([]));
    let caps = provider.capabilities();
    assert!(caps.supports_vision);
    assert!(caps.max_context_tokens > 0);
}

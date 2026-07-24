//! Integration tests for [`OpenAiProvider`] using mockito.
//!
//! All upstream HTTP calls are intercepted by a local mockito server — no real
//! external network access.

use std::sync::Arc;

use futures::StreamExt;
use llm_proxy::config::KeyEntry;
use llm_proxy::error::AppError;
use llm_proxy::provider::Provider;
use llm_proxy::provider::ProviderResponse;
use llm_proxy::provider::inspector::StreamInspector;
use llm_proxy::provider::openai::OpenAiProvider;
use llm_proxy::types::ChatCompletionRequest;
use mockito::ServerGuard;

fn test_provider(server: &ServerGuard) -> OpenAiProvider {
    let codes: Arc<[u16]> = Arc::from([401, 402, 403, 429]);
    OpenAiProvider::new("openai".into(), server.url().to_string(), codes)
}

fn test_key() -> KeyEntry {
    KeyEntry {
        key: "sk-test-key".into(),
        weight: 1,
    }
}

fn test_request() -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "gpt-4o".into(),
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
    }
}

#[tokio::test]
async fn it_returns_once_on_2xx_non_stream() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer sk-test-key")
        .match_header("content-type", "application/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"chat-1","object":"chat.completion","created":123,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"Hello!"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#
        )
        .create_async()
        .await;

    let provider = test_provider(&server);
    let result = provider.chat(test_request(), &test_key()).await.unwrap();

    match result {
        ProviderResponse::Once(resp) => {
            assert_eq!(resp.model, "gpt-4o");
            assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
            let u = resp.usage.unwrap();
            assert_eq!(u.total_tokens, 15);
        }
        ProviderResponse::Stream { .. } => panic!("expected Once, got Stream"),
    }
}

#[tokio::test]
async fn it_returns_stream_on_2xx_sse() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(
            "data: {\"id\":\"1\",\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"}}]}\n\n\
             data: {\"id\":\"2\",\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" there\"}}]}\n\n\
             data: {\"id\":\"3\",\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n\
             data: [DONE]\n\n"
        )
        .create_async()
        .await;

    let provider = test_provider(&server);
    let result = provider.chat(test_request(), &test_key()).await.unwrap();

    match result {
        ProviderResponse::Stream { body } => {
            let mut body = body;
            let mut inspector = StreamInspector::new();
            let mut total_bytes = 0usize;

            while let Some(chunk) = body.next().await {
                let bytes = chunk.unwrap();
                total_bytes += bytes.len();
                for line in bytes.split(|b| *b == b'\n') {
                    inspector.ingest_chunk(line);
                }
            }

            assert!(total_bytes > 0, "stream body should not be empty");

            let summary = inspector.finalize();
            let usage = summary.usage.expect("usage should be accumulated");
            assert_eq!(usage.total_tokens, 7);
            assert_eq!(usage.prompt_tokens, 5);
            assert_eq!(usage.completion_tokens, 2);
            assert_eq!(summary.finish_reason.as_deref(), Some("stop"));
        }
        ProviderResponse::Once(_) => panic!("expected Stream, got Once"),
    }
}

#[tokio::test]
async fn it_returns_bad_key_hint_on_429() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(429)
        .with_body(r#"{"error":{"message":"rate limited"}}"#)
        .create_async()
        .await;

    let provider = test_provider(&server);
    let result = provider.chat(test_request(), &test_key()).await;

    match result {
        Err(AppError::Upstream {
            status,
            retryable,
            bad_key_hint,
            ..
        }) => {
            assert_eq!(status, Some(429));
            assert!(retryable);
            assert!(bad_key_hint);
        }
        other => panic!("expected Upstream error, got {:?}", other),
    }
}

#[tokio::test]
async fn it_returns_no_bad_key_hint_on_503() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(503)
        .with_body("service unavailable")
        .create_async()
        .await;

    let provider = test_provider(&server);
    let result = provider.chat(test_request(), &test_key()).await;

    match result {
        Err(AppError::Upstream {
            status,
            retryable,
            bad_key_hint,
            ..
        }) => {
            assert_eq!(status, Some(503));
            assert!(retryable);
            assert!(!bad_key_hint);
        }
        other => panic!("expected Upstream error, got {:?}", other),
    }
}

#[tokio::test]
async fn it_passes_through_non_bad_4xx() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(404)
        .with_body(r#"{"error":{"message":"not found"}}"#)
        .create_async()
        .await;

    let provider = test_provider(&server);
    let result = provider.chat(test_request(), &test_key()).await;

    match result {
        Err(AppError::Upstream {
            status,
            retryable,
            bad_key_hint,
            ..
        }) => {
            assert_eq!(status, Some(404));
            assert!(!retryable);
            assert!(!bad_key_hint);
        }
        other => panic!("expected Upstream error, got {:?}", other),
    }
}

#[tokio::test]
async fn it_probe_returns_ok_on_2xx() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/models")
        .with_status(200)
        .with_body(r#"{"data":[]}"#)
        .create_async()
        .await;

    let provider = test_provider(&server);
    let result = provider.probe(&test_key()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn it_probe_returns_err_on_non_2xx() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/models")
        .with_status(401)
        .create_async()
        .await;

    let provider = test_provider(&server);
    let result = provider.probe(&test_key()).await;
    assert!(result.is_err());
}

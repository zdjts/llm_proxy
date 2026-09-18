//! End-to-end normalisation tests.
//!
//! These tests exercise the same code path the handler uses for
//! non-streaming ([`normalize_chat_response`]) and streaming
//! ([`NormalizingStream`]).  No real upstream is contacted — `mockito`
//! intercepts every HTTP call.

use bytes::Bytes;
use futures::StreamExt;
use llm_proxy::config::KeyEntry;
use llm_proxy::provider::Provider;
use llm_proxy::provider::ProviderResponse;
use llm_proxy::provider::openai::OpenAiProvider;
use llm_proxy::response_normalize::{ThinkEvent, ThinkTagAccumulator, normalize_chat_response};
use llm_proxy::server::stream_normalize::NormalizingStream;
use llm_proxy::types::ChatCompletionRequest;
use mockito::ServerGuard;
use std::sync::Arc;

fn test_provider(server: &ServerGuard) -> OpenAiProvider {
    let codes: Arc<[u16]> = Arc::from([401, 402, 403, 429]);
    OpenAiProvider::new("openai".into(), server.url().to_string(), codes)
}

fn test_key() -> KeyEntry {
    KeyEntry::api_key("sk-test-key", 1)
}

fn test_request(stream: bool) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: serde_json::from_str(r#"[{"role":"user","content":"hi"}]"#).unwrap(),
        stream: Some(stream),
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
async fn non_streaming_disabled_leaves_content_intact() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"r","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"<think>hidden</think>visible"},"finish_reason":"stop"}]}"#,
        )
        .create_async()
        .await;

    let provider = test_provider(&server);
    let resp = provider
        .chat(test_request(false), &test_key())
        .await
        .unwrap();
    let body = match resp {
        ProviderResponse::Once(b) => b,
        ProviderResponse::Stream { .. } => panic!("expected Once"),
    };

    // Provider path does not normalise — that is the handler's job when
    // the operator has opted in.
    let content = body.choices[0].message.content.as_deref().unwrap_or("");
    assert_eq!(content, "<think>hidden</think>visible");
    assert!(body.choices[0].message.reasoning_content.is_none());
}

#[tokio::test]
async fn non_streaming_enabled_extracts_think_into_reasoning_content() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"r","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"before<think>hidden</think>after"},"finish_reason":"stop"}]}"#,
        )
        .create_async()
        .await;

    let provider = test_provider(&server);
    let resp = provider
        .chat(test_request(false), &test_key())
        .await
        .unwrap();
    let mut body = match resp {
        ProviderResponse::Once(b) => b,
        ProviderResponse::Stream { .. } => panic!("expected Once"),
    };

    normalize_chat_response(&mut body);
    let msg = &body.choices[0].message;
    assert_eq!(msg.content.as_deref(), Some("beforeafter"));
    assert_eq!(msg.reasoning_content.as_deref(), Some("hidden"));
}

#[tokio::test]
async fn non_streaming_folds_reasoning_alias() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"r","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"answer","reasoning":"plan"},"finish_reason":"stop"}]}"#,
        )
        .create_async()
        .await;

    let provider = test_provider(&server);
    let resp = provider
        .chat(test_request(false), &test_key())
        .await
        .unwrap();
    let mut body = match resp {
        ProviderResponse::Once(b) => b,
        ProviderResponse::Stream { .. } => panic!("expected Once"),
    };

    normalize_chat_response(&mut body);
    let msg = &body.choices[0].message;
    assert_eq!(msg.content.as_deref(), Some("answer"));
    assert_eq!(msg.reasoning_content.as_deref(), Some("plan"));
    assert!(msg.reasoning.is_none());
}

#[tokio::test]
async fn non_streaming_multiple_blocks_become_joined_reasoning() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"r","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"<think>A</think>mid<think>B</think>tail"},"finish_reason":"stop"}]}"#,
        )
        .create_async()
        .await;

    let provider = test_provider(&server);
    let resp = provider
        .chat(test_request(false), &test_key())
        .await
        .unwrap();
    let mut body = match resp {
        ProviderResponse::Once(b) => b,
        ProviderResponse::Stream { .. } => panic!("expected Once"),
    };

    normalize_chat_response(&mut body);
    let msg = &body.choices[0].message;
    assert_eq!(msg.content.as_deref(), Some("midtail"));
    assert_eq!(msg.reasoning_content.as_deref(), Some("A\nB"));
}

#[tokio::test]
async fn streaming_split_chunks_resolve_into_correct_reasoning_and_text() {
    let upstream_chunks: Vec<Result<Bytes, llm_proxy::error::AppError>> = vec![
        Ok(Bytes::from(
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"<th\"}}]}\n\n",
        )),
        Ok(Bytes::from(
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ink>sec\"}}]}\n\n",
        )),
        Ok(Bytes::from(
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ret</think>visi\"}}]}\n\n",
        )),
        Ok(Bytes::from(
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ble\"}}]}\n\n",
        )),
        Ok(Bytes::from("data: [DONE]\n\n")),
    ];
    let stream = NormalizingStream::new(futures::stream::iter(upstream_chunks));
    let collected: Vec<Bytes> = stream.filter_map(|r| async move { r.ok() }).collect().await;
    let joined = String::from_utf8(collected.iter().fold(Vec::new(), |mut acc, b| {
        acc.extend_from_slice(b);
        acc
    }))
    .unwrap();

    let mut reasoning = String::new();
    let mut text = String::new();
    for line in joined.lines() {
        if let Some(rest) = line.strip_prefix("data: ") {
            if rest == "[DONE]" {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest) {
                if let Some(s) = v
                    .pointer("/choices/0/delta/reasoning_content")
                    .and_then(|x| x.as_str())
                {
                    reasoning.push_str(s);
                }
                if let Some(s) = v
                    .pointer("/choices/0/delta/content")
                    .and_then(|x| x.as_str())
                {
                    text.push_str(s);
                }
            }
        }
    }
    assert_eq!(reasoning, "secret");
    assert_eq!(text, "visible");
    assert!(
        !joined.contains("<think>") && !joined.contains("</think>"),
        "raw tags must not leak through: {joined}"
    );
    assert!(joined.contains("data: [DONE]"));
}

#[tokio::test]
async fn streaming_unclosed_tag_is_flushed_as_reasoning() {
    let upstream_chunks: Vec<Result<Bytes, llm_proxy::error::AppError>> = vec![Ok(Bytes::from(
        "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"<think>half-formed reasoning body\"}}]}\n\n",
    ))];
    let stream = NormalizingStream::new(futures::stream::iter(upstream_chunks));
    let collected: Vec<Bytes> = stream.filter_map(|r| async move { r.ok() }).collect().await;
    let joined = String::from_utf8(collected.iter().fold(Vec::new(), |mut acc, b| {
        acc.extend_from_slice(b);
        acc
    }))
    .unwrap();
    assert!(
        joined.contains("reasoning_content"),
        "expected reasoning chunk on flush: {joined}"
    );
    assert!(
        joined.contains("half-formed reasoning body"),
        "expected held bytes in reasoning: {joined}"
    );
}

#[tokio::test]
async fn streaming_keeps_native_reasoning_content() {
    let upstream_chunks: Vec<Result<Bytes, llm_proxy::error::AppError>> = vec![Ok(Bytes::from(
        "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"answer\",\"reasoning\":\"plan\"}}]}\n\n",
    ))];
    let stream = NormalizingStream::new(futures::stream::iter(upstream_chunks));
    let collected: Vec<Bytes> = stream.filter_map(|r| async move { r.ok() }).collect().await;
    let joined = String::from_utf8(collected.iter().fold(Vec::new(), |mut acc, b| {
        acc.extend_from_slice(b);
        acc
    }))
    .unwrap();
    assert!(joined.contains(r#""reasoning_content":"plan""#), "{joined}");
    assert!(joined.contains(r#""content":"answer""#), "{joined}");
}

#[tokio::test]
async fn streaming_tag_spans_three_chunks() {
    let upstream_chunks: Vec<Result<Bytes, llm_proxy::error::AppError>> = vec![
        Ok(Bytes::from(
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"<thi\"}}]}\n\n",
        )),
        Ok(Bytes::from(
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"n\"}}]}\n\n",
        )),
        Ok(Bytes::from(
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"k>rest</think>answer\"}}]}\n\n",
        )),
    ];
    let stream = NormalizingStream::new(futures::stream::iter(upstream_chunks));
    let collected: Vec<Bytes> = stream.filter_map(|r| async move { r.ok() }).collect().await;
    let joined = String::from_utf8(collected.iter().fold(Vec::new(), |mut acc, b| {
        acc.extend_from_slice(b);
        acc
    }))
    .unwrap();
    assert!(
        joined.contains(r#""reasoning_content":"rest""#),
        "expected reasoning_content with rest: {joined}"
    );
    assert!(
        joined.contains(r#""content":"answer""#),
        "expected content with answer: {joined}"
    );
}

#[test]
fn accumulator_round_trips_think_then_text() {
    let pairs = vec![("<think>".to_string(), "</think>".to_string())];
    let mut acc = ThinkTagAccumulator::new(pairs);
    let mut reasoning = String::new();
    let mut text = String::new();
    for chunk in ["<think>plan", " A</think>vis", "ible"] {
        for ev in acc.feed(chunk) {
            match ev {
                ThinkEvent::Reasoning(s) => reasoning.push_str(&s),
                ThinkEvent::Text(s) => text.push_str(&s),
                ThinkEvent::Pending => {}
            }
        }
    }
    assert_eq!(reasoning, "plan A");
    assert_eq!(text, "visible");
}

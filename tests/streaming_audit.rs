//! Streaming integration tests — ADR-010 §5 (T38).

use std::sync::Arc;

use llm_proxy::config::KeyEntry;
use llm_proxy::provider::Provider;
use llm_proxy::provider::anthropic::AnthropicProvider;
use llm_proxy::provider::gemini::GeminiProvider;
use llm_proxy::provider::inspector::StreamInspector;
use llm_proxy::types::ChatCompletionRequest;

fn stream_req() -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "test-model".into(),
        messages: serde_json::from_value(serde_json::json!([{"role":"user","content":"hi"}]))
            .unwrap(),
        stream: Some(true),
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

fn test_key() -> KeyEntry {
    KeyEntry {
        key: "sk-test".into(),
        weight: 1,
    }
}

#[tokio::test]
async fn anthropic_connect_error_returns_upstream_err() {
    let codes: Arc<[u16]> = Arc::from([429]);
    let p = AnthropicProvider::new("an".into(), "http://127.0.0.1:1".into(), codes);
    let result = p.chat(stream_req(), &test_key()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn gemini_connect_error_returns_upstream_err() {
    let codes: Arc<[u16]> = Arc::from([429]);
    let p = GeminiProvider::new("gm".into(), "http://127.0.0.1:1".into(), codes);
    let result = p.chat(stream_req(), &test_key()).await;
    assert!(result.is_err());
}

#[test]
fn inspector_accumulates_audit_from_chunks() {
    let mut inspector = StreamInspector::new();
    inspector.ingest_chunk(b"data: {\"id\":\"c1\",\"choices\":[{\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens_details\":{\"cached_tokens\":800},\"completion_tokens_details\":{\"reasoning_tokens\":200}}}\n");
    let audit = inspector.into_audit();
    assert_eq!(audit.cache.hit_tokens, Some(800));
    assert_eq!(audit.reasoning_tokens, Some(200));
    assert_eq!(audit.finish_reason.as_deref(), Some("stop"));
}

#[test]
fn inspector_no_usage_returns_empty_audit() {
    let inspector = StreamInspector::new();
    let audit = inspector.into_audit();
    assert!(audit.cache.hit_tokens.is_none());
    assert!(audit.reasoning_tokens.is_none());
}

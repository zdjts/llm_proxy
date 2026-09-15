//! OpenAI-compatible DTOs for chat completions.
//!
//! All request structs disable `#[serde(deny_unknown_fields)]` to allow transparent
//! pass-through of upstream-specific fields. Response structs are Serialize-only;
//! streaming chunks are both Deserialize (for parsing the incoming SSE stream) and
//! Serialize (for re-emitting unchanged bytes).

use serde::{Deserialize, Serialize};

/// Request body for `POST /v1/chat/completions`.
///
/// Unknown JSON fields are silently ignored to preserve upstream transparency.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<serde_json::Value>,
    #[serde(flatten, default, skip_serializing_if = "serde_json::Value::is_null")]
    pub extra: serde_json::Value,
}

/// A single message in the conversation history.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: String,
    #[serde(default)]
    pub content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Non-streaming chat completion response.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatCompletionResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_chat_object")]
    pub object: String,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
    /// Raw `"usage"` JSON object from the upstream response.  Never serialised.
    /// Populated by the provider after deserialising the body so that
    /// [`extract_cache`](crate::provider::Provider::extract_cache) can inspect
    /// provider-specific cache fields without re-parsing.
    #[serde(skip, default)]
    pub raw_usage_json: Option<serde_json::Value>,
}

/// A single completion choice.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Choice {
    #[serde(default)]
    pub index: u32,
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

/// The assistant message within a non-streaming choice.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResponseMessage {
    #[serde(default = "default_assistant_role")]
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Provider-specific hidden reasoning text, when exposed by the upstream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Provider-specific structured thinking payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<serde_json::Value>,
}

/// A tool call requested by the model in a non-streaming response.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: ToolCallFunction,
}

/// Function-level details of a tool call.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// A single SSE chunk in a streaming chat completion response.
#[derive(Debug, Deserialize, Serialize)]
pub struct ChatCompletionChunk {
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_chunk_object")]
    pub object: String,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub model: String,
    pub choices: Vec<ChunkChoice>,
    pub usage: Option<Usage>,
}

/// A single completion choice inside a streaming chunk.
#[derive(Debug, Deserialize, Serialize)]
pub struct ChunkChoice {
    #[serde(default)]
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

/// The incremental content delta within a streaming chunk choice.
#[derive(Debug, Deserialize, Serialize)]
pub struct Delta {
    pub role: Option<String>,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<DeltaToolCall>>,
    /// Provider-specific incremental reasoning text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Provider-specific incremental thinking payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<serde_json::Value>,
}

/// An incremental tool call within a streaming delta.
#[derive(Debug, Deserialize, Serialize)]
pub struct DeltaToolCall {
    pub index: u32,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub function: Option<DeltaToolCallFunction>,
}

/// Function-level delta of an incremental tool call.
#[derive(Debug, Deserialize, Serialize)]
pub struct DeltaToolCallFunction {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

/// Token usage counters returned by the upstream API.
#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
}

fn default_chat_object() -> String {
    "chat.completion".into()
}

fn default_chunk_object() -> String {
    "chat.completion.chunk".into()
}

fn default_assistant_role() -> String {
    "assistant".into()
}

/// A model entry in the `/v1/models` listing.
#[derive(Debug, Serialize)]
pub struct Model {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

#[derive(Debug, Serialize)]
pub struct ModelMetadataPricing {
    pub input_usd_per_million_tokens: f64,
    pub output_usd_per_million_tokens: f64,
    pub cache_read_usd_per_million_tokens: f64,
    pub cache_write_usd_per_million_tokens: f64,
}

#[derive(Debug, Serialize)]
pub struct ModelMetadata {
    pub id: String,
    pub name: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub input_types: Vec<String>,
    pub reasoning: bool,
    pub thinking_levels: Vec<String>,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub pricing: ModelMetadataPricing,
}

#[derive(Debug, Serialize)]
pub struct ModelMetadataResponse {
    pub object: String,
    pub data: Vec<ModelMetadata>,
}

/// Response payload for `GET /v1/models`.
#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub object: String,
    pub data: Vec<Model>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_deserializes_chat_completion_request_minimal() {
        let json = r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model, "gpt-4o");
        assert!(req.stream.is_none());
        assert!(req.max_tokens.is_none());
    }

    #[test]
    fn it_deserializes_chat_completion_request_full() {
        let json = r#"{
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true,
            "max_tokens": 100,
            "temperature": 0.7,
            "top_p": 0.9,
            "stop": ["\n"],
            "presence_penalty": 0.1,
            "frequency_penalty": 0.1,
            "user": "test-user"
        }"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.stream, Some(true));
        assert_eq!(req.max_tokens, Some(100));
    }

    #[test]
    fn it_ignores_unknown_fields() {
        let json = r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}],"extra_field":"should_not_crash"}"#;
        let req: Result<ChatCompletionRequest, _> = serde_json::from_str(json);
        assert!(req.is_ok());
    }

    #[test]
    fn it_omits_null_extra_when_serializing() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![Message {
                role: "user".into(),
                content: serde_json::Value::String("hi".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
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
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("extra").is_none());
        assert_eq!(json["model"], "gpt-4o");
    }

    #[test]
    fn it_deserializes_chunk() {
        let json = r#"{"id":"chunk-1","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4o","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let chunk: ChatCompletionChunk = serde_json::from_str(json).unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
        assert!(chunk.choices[0].finish_reason.is_none());
    }

    #[test]
    fn it_deserializes_response_missing_created() {
        let json = r#"{"id":"chat-1","object":"chat.completion","model":"glm-5.3-flash","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#;
        let resp: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.created, 0);
        assert_eq!(resp.model, "glm-5.3-flash");
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("hi"));
    }

    #[test]
    fn it_serializes_models_response() {
        let models = vec![Model {
            id: "gpt-4o".into(),
            object: "model".into(),
            created: 1234567890,
            owned_by: "openai".into(),
        }];
        let resp = ModelsResponse {
            object: "list".into(),
            data: models,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("gpt-4o"));
    }

    #[test]
    fn it_handles_tool_calls_in_request_with_null_content() {
        let json = r#"{
            "model":"gpt-4o",
            "messages":[{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"NYC\"}"}}]}]
        }"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.messages[0].role, "assistant");
        assert!(req.messages[0].content.is_null());
        assert!(req.messages[0].tool_calls.is_some());
    }

    #[test]
    fn it_handles_tool_calls_without_content() {
        let json = r#"{
            "model":"gpt-4o",
            "messages":[{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"shell","arguments":"{}"}}]}]
        }"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.messages[0].role, "assistant");
        assert!(req.messages[0].content.is_null());
        assert!(req.messages[0].tool_calls.is_some());
        assert_eq!(
            req.messages[0].tool_calls.as_ref().unwrap()[0]
                .function
                .name,
            "shell"
        );
    }
}

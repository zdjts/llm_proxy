//! Request/Response transform pipeline (Module B2 — v2.0).
//!
//! Transforms are split into pre-request (modify the outgoing request before
//! it reaches the upstream) and post-response (modify the upstream response
//! before it reaches the client).

use crate::error::AppError;
use crate::types::ChatCompletionRequest;

#[derive(Debug, Clone, Default)]
pub struct PipelineConfig {
    pub strip_thinking: bool,
    pub truncate_history: Option<usize>,
    pub system_prompt_inject: Option<String>,
    pub content_filter: bool,
    pub json_repair: bool,
    pub inject_cost_header: bool,
    pub inject_ratelimit_header: bool,
}

/// Pre-request transform: modifies the `ChatCompletionRequest` before upstream dispatch.
pub trait PreRequestTransform: Send + Sync {
    fn name(&self) -> &str;
    fn apply(&self, req: &mut ChatCompletionRequest) -> Result<(), AppError>;
}

/// Post-response transform: modifies response headers or body.
pub trait PostResponseTransform: Send + Sync {
    fn name(&self) -> &str;

    fn apply_headers(&self, _headers: &mut axum::http::HeaderMap) -> Result<(), AppError> {
        Ok(())
    }

    fn apply_body(&self, _body: &mut serde_json::Value) -> Result<(), AppError> {
        Ok(())
    }
}

/// Pre-request: strips thinking/reasoning content from messages.
pub struct StripThinkingTransform;

impl PreRequestTransform for StripThinkingTransform {
    fn name(&self) -> &str {
        "strip_thinking"
    }

    fn apply(&self, req: &mut ChatCompletionRequest) -> Result<(), AppError> {
        for msg in &mut req.messages {
            if let serde_json::Value::Array(ref parts) = msg.content {
                let filtered: Vec<serde_json::Value> = parts
                    .iter()
                    .filter(|p| {
                        p.get("type")
                            .and_then(|t| t.as_str())
                            .is_some_and(|t| t != "thinking")
                    })
                    .cloned()
                    .collect();
                msg.content = serde_json::Value::Array(filtered);
            }
        }
        Ok(())
    }
}

/// Pre-request: truncates message history to last N messages.
pub struct TruncateHistoryTransform {
    pub max_messages: usize,
}

impl PreRequestTransform for TruncateHistoryTransform {
    fn name(&self) -> &str {
        "truncate_history"
    }

    fn apply(&self, req: &mut ChatCompletionRequest) -> Result<(), AppError> {
        if req.messages.len() > self.max_messages {
            let system_msgs: Vec<_> = req
                .messages
                .iter()
                .filter(|m| m.role == "system")
                .cloned()
                .collect();
            let non_system: Vec<_> = req
                .messages
                .iter()
                .filter(|m| m.role != "system")
                .cloned()
                .collect();
            let keep = self.max_messages.saturating_sub(system_msgs.len());
            let truncated: Vec<_> = non_system.into_iter().rev().take(keep).rev().collect();
            req.messages = system_msgs.into_iter().chain(truncated).collect();
        }
        Ok(())
    }
}

/// Pre-request: injects a system prompt as the first message.
pub struct SystemPromptInjectTransform {
    pub prompt: String,
}

impl PreRequestTransform for SystemPromptInjectTransform {
    fn name(&self) -> &str {
        "system_prompt_inject"
    }

    fn apply(&self, req: &mut ChatCompletionRequest) -> Result<(), AppError> {
        if req.messages.first().is_some_and(|m| m.role == "system") {
            return Ok(());
        }
        let sys_msg = crate::types::Message {
            role: "system".into(),
            content: serde_json::Value::String(self.prompt.clone()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        };
        req.messages.insert(0, sys_msg);
        Ok(())
    }
}

/// Post-response: attempts basic JSON repair on content strings.
pub struct JsonRepairTransform;

impl PostResponseTransform for JsonRepairTransform {
    fn name(&self) -> &str {
        "json_repair"
    }

    fn apply_body(&self, body: &mut serde_json::Value) -> Result<(), AppError> {
        if let Some(choices) = body.get_mut("choices").and_then(|c| c.as_array_mut()) {
            for choice in choices {
                if let Some(msg) = choice.get_mut("message").and_then(|m| m.as_object_mut())
                    && let Some(content) = msg.get("content").and_then(|c| c.as_str())
                {
                    let trimmed = content.trim();
                    if ((trimmed.starts_with('{') && trimmed.ends_with('}'))
                        || (trimmed.starts_with('[') && trimmed.ends_with(']')))
                        && let Ok(repaired) = serde_json::from_str::<serde_json::Value>(trimmed)
                    {
                        msg.insert(
                            "content".to_owned(),
                            serde_json::Value::String(
                                serde_json::to_string(&repaired).unwrap_or_default(),
                            ),
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

/// Runs the pre-request transform chain.
pub struct Pipeline {
    pre_transforms: Vec<Box<dyn PreRequestTransform>>,
    post_transforms: Vec<Box<dyn PostResponseTransform>>,
}

impl Pipeline {
    pub fn from_config(config: &PipelineConfig) -> Self {
        let mut pre: Vec<Box<dyn PreRequestTransform>> = Vec::new();
        let mut post: Vec<Box<dyn PostResponseTransform>> = Vec::new();

        if config.strip_thinking {
            pre.push(Box::new(StripThinkingTransform));
        }
        if let Some(max) = config.truncate_history {
            pre.push(Box::new(TruncateHistoryTransform { max_messages: max }));
        }
        if let Some(ref prompt) = config.system_prompt_inject {
            pre.push(Box::new(SystemPromptInjectTransform {
                prompt: prompt.clone(),
            }));
        }
        if config.json_repair {
            post.push(Box::new(JsonRepairTransform));
        }

        Self {
            pre_transforms: pre,
            post_transforms: post,
        }
    }

    pub fn apply_pre(&self, req: &mut ChatCompletionRequest) -> Result<(), AppError> {
        for t in &self.pre_transforms {
            t.apply(req)?;
        }
        Ok(())
    }

    pub fn apply_post_headers(&self, headers: &mut axum::http::HeaderMap) -> Result<(), AppError> {
        for t in &self.post_transforms {
            t.apply_headers(headers)?;
        }
        Ok(())
    }

    pub fn apply_post_body(&self, body: &mut serde_json::Value) -> Result<(), AppError> {
        for t in &self.post_transforms {
            t.apply_body(body)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    #[test]
    fn it_injects_system_prompt() {
        let mut req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![Message {
                role: "user".into(),
                content: serde_json::Value::String("hi".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: None,
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

        let t = SystemPromptInjectTransform {
            prompt: "You are helpful.".into(),
        };
        t.apply(&mut req).unwrap();
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
    }

    #[test]
    fn it_truncates_history() {
        let msgs: Vec<Message> = (0..10)
            .map(|i| Message {
                role: "user".into(),
                content: serde_json::Value::String(format!("msg{i}")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            })
            .collect();

        let mut req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: msgs,
            stream: None,
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

        let t = TruncateHistoryTransform { max_messages: 5 };
        t.apply(&mut req).unwrap();
        assert_eq!(req.messages.len(), 5);
        assert_eq!(
            req.messages[4].content,
            serde_json::Value::String("msg9".into())
        );
    }
}

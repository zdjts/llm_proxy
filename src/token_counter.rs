//! Token counting utilities — character-based heuristics for estimation.
//!
//! Rough estimates based on character/token ratio (typically ~4 chars = 1 token for English,
//! ~2 chars = 1 token for CJK). Useful for pre-request cost estimation and rate limiting.

use crate::types::ChatCompletionRequest;

/// Rough token count for a string based on character-level heuristics.
pub fn estimate_tokens(text: &str) -> u32 {
    let mut tokens: u32 = 0;
    for ch in text.chars() {
        if ch.is_ascii_whitespace() {
            tokens += 1;
        } else if ch.is_ascii() {
            // English text: ~4 chars per token
            tokens += 1;
        } else {
            // CJK and other wide chars: ~2 chars per token
            tokens += 2;
        }
    }
    // Divide by 4 for English rough ratio
    tokens / 4
}

/// Estimate total prompt tokens for a chat completion request.
pub fn estimate_request_tokens(req: &ChatCompletionRequest) -> u32 {
    let mut total = 0u32;
    for msg in &req.messages {
        // Count role as 2 tokens
        total += 2;
        // Count content
        match &msg.content {
            serde_json::Value::String(s) => {
                total += estimate_tokens(s);
            }
            serde_json::Value::Array(parts) => {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        total += estimate_tokens(text);
                    }
                    if let Some(img_url) = part
                        .get("image_url")
                        .and_then(|v| v.get("url"))
                        .and_then(|v| v.as_str())
                    {
                        // Vision model: roughly 85-170 tokens per image (base85)
                        total += if img_url.starts_with("data:") { 85 } else { 20 };
                    }
                }
            }
            _ => {
                let s = msg.content.to_string();
                total += estimate_tokens(&s);
            }
        }
        // Tool calls
        if let Some(ref tool_calls) = msg.tool_calls {
            for tc in tool_calls {
                total += estimate_tokens(&tc.function.name);
                total += estimate_tokens(&tc.function.arguments);
                total += 8; // structural overhead per tool call
            }
        }
    }
    // System overhead
    total += 4;
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_text_estimate() {
        let tokens = estimate_tokens("Hello, how are you today?");
        assert!(tokens > 0);
        assert!(tokens < 20);
    }

    #[test]
    fn empty_string_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn request_estimation_nonzero() {
        use crate::types::Message;
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![Message {
                role: "user".into(),
                content: serde_json::Value::String("Hello world".into()),
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
        let tokens = estimate_request_tokens(&req);
        assert!(tokens >= 2);
    }
}

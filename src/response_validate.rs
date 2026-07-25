//! Response validation — ensures upstream responses conform to OpenAI schema.
//!
//! Runs as a non-blocking validation pass; invalid responses are logged as warnings
//! but never rejected (fail-open for reliability).

use crate::types::ChatCompletionResponse;

/// Validate the structure of an upstream response.
/// Returns a list of validation warnings (empty = valid).
pub fn validate_response(resp: &ChatCompletionResponse) -> Vec<String> {
    let mut warnings = Vec::new();

    if resp.id.is_empty() {
        warnings.push("missing response id".into());
    }

    if resp.object != "chat.completion" && resp.object != "chat.completion.chunk" {
        warnings.push(format!(
            "unexpected response object type: '{}'",
            resp.object
        ));
    }

    if resp.created == 0 {
        warnings.push("missing or zero created timestamp".into());
    }

    if resp.model.is_empty() {
        warnings.push("missing model in response".into());
    }

    if resp.choices.is_empty() {
        warnings.push("response has no choices".into());
    }

    for (i, choice) in resp.choices.iter().enumerate() {
        if choice.message.role.is_empty() {
            warnings.push(format!("choice[{i}] has empty role"));
        }
        if choice.message.content.is_none() && choice.message.tool_calls.is_none() {
            warnings.push(format!("choice[{i}] has neither content nor tool_calls"));
        }
    }

    if let Some(ref usage) = resp.usage {
        let computed_total = usage.prompt_tokens + usage.completion_tokens;
        if usage.total_tokens != 0 && usage.total_tokens != computed_total {
            warnings.push(format!(
                "usage total_tokens ({}) != prompt ({}) + completion ({})",
                usage.total_tokens, usage.prompt_tokens, usage.completion_tokens
            ));
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Choice, ResponseMessage, Usage};

    fn valid_response() -> ChatCompletionResponse {
        ChatCompletionResponse {
            id: "chatcmpl-123".into(),
            object: "chat.completion".into(),
            created: 1234567890,
            model: "gpt-4o".into(),
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            raw_usage_json: None,
        }
    }

    #[test]
    fn valid_response_no_warnings() {
        let warnings = validate_response(&valid_response());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn empty_id_produces_warning() {
        let mut resp = valid_response();
        resp.id = String::new();
        let warnings = validate_response(&resp);
        assert!(!warnings.is_empty());
    }

    #[test]
    fn no_choices_produces_warning() {
        let mut resp = valid_response();
        resp.choices = vec![];
        let warnings = validate_response(&resp);
        assert!(!warnings.is_empty());
    }

    #[test]
    fn usage_mismatch_produces_warning() {
        let mut resp = valid_response();
        resp.usage = Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 999,
        });
        let warnings = validate_response(&resp);
        assert!(!warnings.is_empty());
    }
}

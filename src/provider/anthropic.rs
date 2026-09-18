//! Anthropic Messages API provider (ADR-008 §4).
//!
//! Converts OpenAI-formatted `ChatCompletionRequest` ↔ Anthropic native Messages API.
//! Non-streaming only in this commit; streaming lands in T22.

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};

use crate::audit::{AuditFromProvider, CacheReport, ProviderCacheKind};
use crate::config::KeyEntry;
use crate::error::AppError;
use crate::provider::{Provider, ProviderResponse};
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, Choice, ResponseMessage, Usage};

pub struct AnthropicProvider {
    id: String,
    base_url: String,
    client: reqwest::Client,
    bad_status_codes: Arc<[u16]>,
}

impl AnthropicProvider {
    pub fn new(id: String, base_url: String, bad_status_codes: Arc<[u16]>) -> Self {
        Self {
            id,
            base_url,
            client: crate::provider::http::build_http_client(),
            bad_status_codes,
        }
    }

    fn is_bad_status(&self, s: u16) -> bool {
        self.bad_status_codes.contains(&s)
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn chat(
        &self,
        req: ChatCompletionRequest,
        key: &KeyEntry,
    ) -> Result<ProviderResponse, AppError> {
        let stream = req.stream.unwrap_or(false);
        if stream {
            let an_req = openai_to_anthropic_stream(&req);
            let url = format!("{}/messages", self.base_url);
            let resp = self
                .client
                .post(&url)
                .header("x-api-key", &key.key)
                .header("anthropic-version", "2023-06-01")
                .json(&an_req)
                .send()
                .await
                .map_err(|e| AppError::Upstream {
                    status: None,
                    retryable: true,
                    bad_key_hint: false,
                    msg: format!("stream connect: {e}"),
                })?;
            let s = resp.status().as_u16();
            if !resp.status().is_success() {
                return Err(AppError::Upstream {
                    status: Some(s),
                    retryable: s >= 500,
                    bad_key_hint: self.is_bad_status(s),
                    msg: crate::provider::http::upstream_error_message(s, resp).await,
                });
            }
            let raw_stream = Box::pin(resp.bytes_stream().map(|item| match item {
                Ok(b) => Ok(b),
                Err(e) => Err(AppError::Upstream {
                    status: None,
                    retryable: true,
                    bad_key_hint: false,
                    msg: format!("stream error: {e}"),
                }),
            }));
            let body = super::anthropic_stream::relay_anthropic_stream(raw_stream, &req.model);
            return Ok(ProviderResponse::Stream { body });
        }
        let an_req = openai_to_anthropic(&req);
        let url = format!("{}/messages", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &key.key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&an_req)
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                return Err(AppError::Upstream {
                    status: None,
                    retryable: true,
                    bad_key_hint: false,
                    msg: format!("anthropic connect error: {e}"),
                });
            }
        };

        let status = response.status().as_u16();

        if response.status().is_success() {
            let raw_body: serde_json::Value =
                response.json().await.map_err(|e| AppError::Upstream {
                    status: Some(status),
                    retryable: false,
                    bad_key_hint: false,
                    msg: format!("parse error: {e}"),
                })?;
            let an_resp: AnthropicResponse =
                serde_json::from_value(raw_body.clone()).map_err(|e| AppError::Upstream {
                    status: Some(status),
                    retryable: false,
                    bad_key_hint: false,
                    msg: format!("deserialize error: {e}"),
                })?;
            let mut chat_resp = anthropic_to_openai(&an_resp, &req.model);
            chat_resp.raw_usage_json = raw_body.get("usage").cloned();
            Ok(ProviderResponse::Once(chat_resp))
        } else if status >= 500 {
            Err(AppError::Upstream {
                status: Some(status),
                retryable: true,
                bad_key_hint: false,
                msg: crate::provider::http::upstream_error_message(status, response).await,
            })
        } else if self.is_bad_status(status) {
            Err(AppError::Upstream {
                status: Some(status),
                retryable: true,
                bad_key_hint: true,
                msg: crate::provider::http::upstream_error_message(status, response).await,
            })
        } else {
            Err(AppError::Upstream {
                status: Some(status),
                retryable: false,
                bad_key_hint: false,
                msg: crate::provider::http::upstream_error_message(status, response).await,
            })
        }
    }

    async fn probe(&self, key: &KeyEntry) -> Result<(), AppError> {
        let url = format!("{}/messages/count_tokens", self.base_url);
        let body = serde_json::json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [{"role":"user","content":"."}]
        });
        let r = self
            .client
            .post(&url)
            .header("x-api-key", &key.key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Upstream {
                status: None,
                retryable: true,
                bad_key_hint: false,
                msg: format!("probe error: {e}"),
            })?;
        if r.status().is_success() {
            Ok(())
        } else {
            Err(AppError::Upstream {
                status: Some(r.status().as_u16()),
                retryable: true,
                bad_key_hint: false,
                msg: format!("probe: {}", r.status()),
            })
        }
    }

    fn extract_audit(&self, resp: &ProviderResponse) -> AuditFromProvider {
        match resp {
            ProviderResponse::Once(chat_resp) => {
                let mut audit = AuditFromProvider::default();
                if let Some(ref raw) = chat_resp.raw_usage_json {
                    let hit = raw.get("cache_read_input_tokens").and_then(|v| v.as_i64());
                    let creation = raw
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_i64());
                    if hit.is_some() || creation.is_some() {
                        audit.cache = CacheReport {
                            hit_tokens: hit,
                            creation_tokens: creation,
                            source: ProviderCacheKind::AnthropicCacheControl,
                        };
                    }
                }
                audit.upstream_model = Some(chat_resp.model.clone());
                audit.finish_reason = chat_resp
                    .choices
                    .first()
                    .and_then(|c| c.finish_reason.clone());
                audit
            }
            ProviderResponse::Stream { .. } => AuditFromProvider::default(),
        }
    }
}

// ── DTOs ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<serde_json::Value>,
    stream: bool,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<AnthropicContent>,
    stop_reason: Option<String>,
    usage: serde_json::Value,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    type_: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
}

// ── Conversion ────────────────────────────────────────────────────────────

fn openai_to_anthropic(req: &ChatCompletionRequest) -> AnthropicRequest {
    openai_to_anthropic_base(req, false)
}

fn openai_to_anthropic_stream(req: &ChatCompletionRequest) -> AnthropicRequest {
    openai_to_anthropic_base(req, true)
}

fn openai_to_anthropic_base(req: &ChatCompletionRequest, stream: bool) -> AnthropicRequest {
    let mut system_parts: Vec<String> = Vec::new();
    let mut messages = Vec::new();

    for m in &req.messages {
        match m.role.as_str() {
            "system" => {
                if let serde_json::Value::String(ref s) = m.content {
                    system_parts.push(s.clone());
                }
            }
            "user" | "assistant" => {
                let text = match &m.content {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                messages.push(AnthropicMessage {
                    role: m.role.clone(),
                    content: text,
                });
            }
            _ => {}
        }
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    let max_tokens = req.max_tokens.unwrap_or(4096);

    let stop: Option<Vec<String>> = match &req.stop {
        Some(serde_json::Value::String(s)) => Some(vec![s.clone()]),
        Some(serde_json::Value::Array(arr)) => {
            let v: Vec<String> = arr
                .iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect();
            if v.is_empty() { None } else { Some(v) }
        }
        _ => None,
    };

    let thinking = req
        .extra
        .get("thinking")
        .cloned()
        .or_else(|| req.extra.get("thinking_config").cloned());

    AnthropicRequest {
        model: req.model.clone(),
        max_tokens,
        system,
        messages,
        stop_sequences: stop,
        temperature: req.temperature,
        top_p: req.top_p,
        thinking,
        stream,
    }
}

fn anthropic_to_openai(an: &AnthropicResponse, model: &str) -> ChatCompletionResponse {
    let mut reasoning_parts = Vec::new();
    let content_text: String = an
        .content
        .iter()
        .filter_map(|c| {
            if c.type_ == "thinking" {
                if let Some(text) = c.thinking.as_deref() {
                    reasoning_parts.push(text.to_owned());
                }
                None
            } else if c.type_ == "text" {
                c.text.as_deref()
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("");
    let reasoning_content = if reasoning_parts.is_empty() {
        None
    } else {
        Some(reasoning_parts.join(""))
    };

    let finish_reason = match an.stop_reason.as_deref() {
        Some("end_turn") => Some("stop".into()),
        Some("max_tokens") => Some("length".into()),
        Some("stop_sequence") => Some("stop".into()),
        Some("tool_use") => Some("tool_calls".into()),
        _ => None,
    };

    let usage = Usage {
        prompt_tokens: an
            .usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        completion_tokens: an
            .usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        total_tokens: 0,
    };
    let total = Usage {
        total_tokens: usage.prompt_tokens + usage.completion_tokens,
        ..usage
    };

    ChatCompletionResponse {
        id: an.id.clone(),
        object: "chat.completion".into(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        model: model.into(),
        choices: vec![Choice {
            index: 0,
            message: ResponseMessage {
                role: "assistant".into(),
                content: Some(content_text),
                tool_calls: None,
                reasoning_content,
                reasoning: None,
                reasoning_text: None,
                thinking: None,
            },
            finish_reason,
        }],
        usage: Some(total),
        raw_usage_json: None,
    }
}

//! Gemini `generateContent` API provider (ADR-008 §5).
//!
//! Converts OpenAI-formatted `ChatCompletionRequest` ↔ Gemini native API.
//! Non-streaming only in this commit; streaming lands in T23.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};

use crate::audit::{AuditFromProvider, CacheReport, ProviderCacheKind};
use crate::config::KeyEntry;
use crate::error::AppError;
use crate::provider::{Provider, ProviderResponse};
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, Choice, ResponseMessage, Usage};

pub struct GeminiProvider {
    id: String,
    base_url: String,
    client: reqwest::Client,
    bad_status_codes: Arc<[u16]>,
}

impl GeminiProvider {
    pub fn new(id: String, base_url: String, bad_status_codes: Arc<[u16]>) -> Self {
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(32)
            .timeout(Duration::from_secs(120))
            .user_agent("llm_proxy/0.2")
            .build()
            .expect("reqwest client builder");
        Self {
            id,
            base_url,
            client,
            bad_status_codes,
        }
    }

    fn is_bad_status(&self, s: u16) -> bool {
        self.bad_status_codes.contains(&s)
    }
}

#[async_trait]
impl Provider for GeminiProvider {
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
            let gm_req = openai_to_gemini(&req);
            let url = format!(
                "{}/models/{}:streamGenerateContent?alt=sse",
                self.base_url, req.model
            );
            let resp = self
                .client
                .post(&url)
                .header("x-goog-api-key", &key.key)
                .json(&gm_req)
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
                    msg: format!("upstream {s}"),
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
            let body = super::gemini_stream::relay_gemini_stream(raw_stream, &req.model);
            return Ok(ProviderResponse::Stream { body });
        }
        let gm_req = openai_to_gemini(&req);
        let url = format!("{}/models/{}:generateContent", self.base_url, req.model);

        let response = self
            .client
            .post(&url)
            .header("x-goog-api-key", &key.key)
            .header("content-type", "application/json")
            .json(&gm_req)
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                return Err(AppError::Upstream {
                    status: None,
                    retryable: true,
                    bad_key_hint: false,
                    msg: format!("gemini connect error: {e}"),
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
            let gm_resp: GeminiResponse =
                serde_json::from_value(raw_body.clone()).map_err(|e| AppError::Upstream {
                    status: Some(status),
                    retryable: false,
                    bad_key_hint: false,
                    msg: format!("deserialize error: {e}"),
                })?;
            let mut chat_resp = gemini_to_openai(&gm_resp, &req.model);
            chat_resp.raw_usage_json = raw_body.get("usageMetadata").cloned();
            Ok(ProviderResponse::Once(chat_resp))
        } else if status >= 500 {
            Err(AppError::Upstream {
                status: Some(status),
                retryable: true,
                bad_key_hint: false,
                msg: format!("upstream {status}"),
            })
        } else if self.is_bad_status(status) {
            Err(AppError::Upstream {
                status: Some(status),
                retryable: true,
                bad_key_hint: true,
                msg: format!("upstream {status}"),
            })
        } else {
            Err(AppError::Upstream {
                status: Some(status),
                retryable: false,
                bad_key_hint: false,
                msg: format!("upstream {status}"),
            })
        }
    }

    async fn probe(&self, key: &KeyEntry) -> Result<(), AppError> {
        let url = format!("{}/models", self.base_url);
        let r = self
            .client
            .get(&url)
            .header("x-goog-api-key", &key.key)
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
                if let Some(ref raw) = chat_resp.raw_usage_json
                    && let Some(cached) =
                        raw.get("cachedContentTokenCount").and_then(|v| v.as_i64())
                {
                    audit.cache = CacheReport {
                        hit_tokens: Some(cached),
                        creation_tokens: None,
                        source: ProviderCacheKind::GeminiCachedContent,
                    };
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
#[allow(non_snake_case)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[allow(non_snake_case)]
    systemInstruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generationConfig: Option<GeminiGenerationConfig>,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Deserialize, Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
#[allow(non_snake_case)]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    maxOutputTokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(rename = "topP", skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stopSequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinkingConfig: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[allow(dead_code, non_snake_case)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    usageMetadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct GeminiCandidate {
    content: GeminiContentResponse,
    finishReason: Option<String>,
}

#[derive(Deserialize)]
struct GeminiContentResponse {
    parts: Vec<GeminiResponsePart>,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thought: bool,
}

// ── Conversion ────────────────────────────────────────────────────────────

fn openai_to_gemini(req: &ChatCompletionRequest) -> GeminiRequest {
    let mut system_texts: Vec<String> = Vec::new();
    let mut contents = Vec::new();

    for m in &req.messages {
        match m.role.as_str() {
            "system" => {
                if let serde_json::Value::String(ref s) = m.content {
                    system_texts.push(s.clone());
                }
            }
            "user" => {
                let text = match &m.content {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                contents.push(GeminiContent {
                    role: "user".into(),
                    parts: vec![GeminiPart { text }],
                });
            }
            "assistant" => {
                let text = match &m.content {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                contents.push(GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart { text }],
                });
            }
            _ => {}
        }
    }

    let system_instruction = if system_texts.is_empty() {
        None
    } else {
        Some(GeminiSystemInstruction {
            parts: vec![GeminiPart {
                text: system_texts.join("\n\n"),
            }],
        })
    };

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

    let thinking_config = req
        .extra
        .get("thinkingConfig")
        .cloned()
        .or_else(|| req.extra.get("thinking_config").cloned())
        .or_else(|| req.extra.get("thinking").cloned());

    GeminiRequest {
        contents,
        systemInstruction: system_instruction,
        generationConfig: Some(GeminiGenerationConfig {
            maxOutputTokens: Some(req.max_tokens.unwrap_or(8192)),
            temperature: req.temperature,
            top_p: req.top_p,
            stopSequences: stop,
            thinkingConfig: thinking_config,
        }),
    }
}

fn gemini_to_openai(gm: &GeminiResponse, model: &str) -> ChatCompletionResponse {
    let candidate = gm.candidates.first();
    let mut reasoning_parts = Vec::new();
    let mut answer_parts = Vec::new();
    if let Some(candidate) = candidate {
        for part in &candidate.content.parts {
            if let Some(text) = part.text.as_deref() {
                if part.thought {
                    reasoning_parts.push(text);
                } else {
                    answer_parts.push(text);
                }
            }
        }
    }
    let content_text = answer_parts.join("");
    let reasoning_content = if reasoning_parts.is_empty() {
        None
    } else {
        Some(reasoning_parts.join(""))
    };

    let finish_reason = candidate
        .and_then(|c| c.finishReason.as_deref())
        .map(|fr| match fr {
            "STOP" => "stop",
            "MAX_TOKENS" => "length",
            "SAFETY" | "RECITATION" => "content_filter",
            _ => "stop",
        })
        .map(String::from);

    let usage = gm.usageMetadata.as_ref();
    let prompt_tokens = usage
        .and_then(|u| u.get("promptTokenCount").and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;
    let completion_tokens = usage
        .and_then(|u| u.get("candidatesTokenCount").and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;
    let total_from_field = usage
        .and_then(|u| u.get("totalTokenCount").and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;
    let total_tokens = if total_from_field > 0 {
        total_from_field
    } else {
        prompt_tokens + completion_tokens
    };

    ChatCompletionResponse {
        id: format!("chatcmpl-{:016x}", rand::random::<u64>()),
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
                thinking: None,
            },
            finish_reason,
        }],
        usage: Some(Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        }),
        raw_usage_json: None,
    }
}

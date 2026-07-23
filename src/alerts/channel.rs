//! AlertChannel trait and delivery implementations — ADR-013 §3 (T55).
//!
//! Four channel types: Webhook, Slack, Discord, Email (stub).

use async_trait::async_trait;
use reqwest::Client;

use super::AlertEvent;

/// Dedicated error for channel delivery failures.
#[derive(Debug)]
pub struct ChannelError {
    pub channel: String,
    pub msg: String,
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.channel, self.msg)
    }
}

impl std::error::Error for ChannelError {}

/// A dispatch target for alert events.
#[async_trait]
pub trait AlertChannel: Send + Sync {
    fn id(&self) -> &str;
    async fn deliver(
        &self,
        event: &AlertEvent,
        signature: Option<&str>,
    ) -> Result<(), ChannelError>;
}

// ── WebhookChannel ──────────────────────────────────────────────────────

pub struct WebhookChannel {
    pub url: String,
    pub secret: String,
}

#[async_trait]
impl AlertChannel for WebhookChannel {
    fn id(&self) -> &str {
        "webhook"
    }

    async fn deliver(&self, event: &AlertEvent, _sig: Option<&str>) -> Result<(), ChannelError> {
        let body = serde_json::to_string(event).unwrap_or_default();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut req = Client::new()
            .post(&self.url)
            .header("content-type", "application/json");

        if !self.secret.is_empty() {
            let sig = super::sign_body(&self.secret, ts, body.as_bytes());
            req = req.header("x-llm-proxy-signature", format!("t={ts},v1={sig}"));
        }

        let resp = req.body(body).send().await.map_err(|e| ChannelError {
            channel: "webhook".into(),
            msg: e.to_string(),
        })?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ChannelError {
                channel: "webhook".into(),
                msg: format!("HTTP {}", resp.status().as_u16()),
            })
        }
    }
}

// ── SlackChannel ────────────────────────────────────────────────────────

pub struct SlackChannel {
    pub url: String,
}

#[async_trait]
impl AlertChannel for SlackChannel {
    fn id(&self) -> &str {
        "slack"
    }

    async fn deliver(&self, event: &AlertEvent, _sig: Option<&str>) -> Result<(), ChannelError> {
        let text = format!(
            "[llm_proxy] {} — {}",
            event.event_type(),
            event.msg_str().unwrap_or("-")
        );
        let body = serde_json::json!({"text": text}).to_string();

        let resp = Client::new()
            .post(&self.url)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| ChannelError {
                channel: "slack".into(),
                msg: e.to_string(),
            })?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ChannelError {
                channel: "slack".into(),
                msg: format!("HTTP {}", resp.status().as_u16()),
            })
        }
    }
}

// ── DiscordChannel ──────────────────────────────────────────────────────

pub struct DiscordChannel {
    pub url: String,
}

#[async_trait]
impl AlertChannel for DiscordChannel {
    fn id(&self) -> &str {
        "discord"
    }

    async fn deliver(&self, event: &AlertEvent, _sig: Option<&str>) -> Result<(), ChannelError> {
        let content = format!(
            "[llm_proxy] {} — {}",
            event.event_type(),
            event.msg_str().unwrap_or("-")
        );
        let body = serde_json::json!({"content": content}).to_string();

        let resp = Client::new()
            .post(&self.url)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| ChannelError {
                channel: "discord".into(),
                msg: e.to_string(),
            })?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ChannelError {
                channel: "discord".into(),
                msg: format!("HTTP {}", resp.status().as_u16()),
            })
        }
    }
}

// ── EmailChannel (stub) ─────────────────────────────────────────────────

pub struct EmailChannel {
    pub to: String,
}

#[async_trait]
impl AlertChannel for EmailChannel {
    fn id(&self) -> &str {
        "email"
    }

    async fn deliver(&self, event: &AlertEvent, _sig: Option<&str>) -> Result<(), ChannelError> {
        tracing::info!(
            to = %self.to,
            event_type = event.event_type(),
            "email stub — would send alert"
        );
        Ok(())
    }
}

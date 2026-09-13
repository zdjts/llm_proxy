//! xAI SuperGrok / X Premium device-code OAuth (RFC 8628).
//!
//! Talks to the public xAI auth server (`auth.x.ai`). Access tokens are used
//! as Bearer credentials against `https://api.x.ai/v1`.

use serde::Deserialize;
use std::time::Duration;

use super::{DeviceCodePrompt, LiveToken, XaiOAuthEndpoints, now_ms};
use crate::config::KeyEntry;
use crate::error::AppError;

/// Public xAI OAuth client used by Grok CLI / subscription logins (no secret).
pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
pub const DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
pub const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";

const REFRESH_SKEW_MS: i64 = 5 * 60 * 1000;
const DEFAULT_TOKEN_LIFETIME_SECS: i64 = 3600;
const DEFAULT_POLL_INTERVAL_SECS: u64 = 5;
const SLOW_DOWN_INCREMENT_SECS: u64 = 5;

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    verification_uri_complete: Option<String>,
    interval: Option<u64>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

pub async fn refresh_token(
    http: &reqwest::Client,
    endpoints: &XaiOAuthEndpoints,
    refresh: &str,
) -> Result<LiveToken, AppError> {
    let resp = http
        .post(&endpoints.token_url)
        .header("Accept", "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", endpoints.client_id.as_str()),
            ("refresh_token", refresh),
        ])
        .send()
        .await
        .map_err(|e| oauth_upstream(format!("xAI token refresh request failed: {e}")))?;

    let status = resp.status();
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| oauth_upstream(format!("xAI token refresh returned invalid JSON: {e}")))?;

    if !status.is_success() {
        return Err(oauth_denied(status.as_u16(), &body));
    }
    credentials_from_token(&body, Some(refresh))
}

pub async fn login(
    http: &reqwest::Client,
    endpoints: &XaiOAuthEndpoints,
    mut notify: impl FnMut(&DeviceCodePrompt),
) -> Result<KeyEntry, AppError> {
    let device = request_device_code(http, endpoints).await?;
    notify(&DeviceCodePrompt {
        user_code: device.user_code.clone(),
        verification_uri: device
            .verification_uri_complete
            .clone()
            .unwrap_or_else(|| device.verification_uri.clone()),
        expires_in_seconds: device.expires_in_seconds,
    });
    let live = poll_for_tokens(http, endpoints, &device).await?;
    Ok(KeyEntry::oauth(
        live.access,
        live.refresh,
        "xai",
        1,
        Some(live.expires),
    ))
}

struct DeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    interval_seconds: u64,
    expires_in_seconds: u64,
}

async fn request_device_code(
    http: &reqwest::Client,
    endpoints: &XaiOAuthEndpoints,
) -> Result<DeviceCode, AppError> {
    let resp = http
        .post(&endpoints.device_code_url)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", endpoints.client_id.as_str()),
            ("scope", SCOPE),
            ("referrer", "llm_proxy"),
        ])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("xAI device authorization request failed: {e}")))?;

    let status = resp.status();
    let body: DeviceCodeResponse = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("xAI device authorization invalid JSON: {e}")))?;

    if !status.is_success() {
        return Err(AppError::Internal(format_oauth_error(
            "device authorization",
            status.as_u16(),
            body.error.as_deref(),
            body.error_description.as_deref(),
        )));
    }

    let verification_uri = required_https(body.verification_uri.as_deref(), "verification_uri")?;
    let verification_uri_complete = match body.verification_uri_complete.as_deref() {
        Some(raw) if !raw.is_empty() => {
            Some(required_https(Some(raw), "verification_uri_complete")?)
        }
        _ => None,
    };

    Ok(DeviceCode {
        device_code: required_string(body.device_code, "device_code")?,
        user_code: required_string(body.user_code, "user_code")?,
        verification_uri,
        verification_uri_complete,
        interval_seconds: body
            .interval
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_POLL_INTERVAL_SECS),
        expires_in_seconds: body.expires_in.filter(|v| *v > 0).ok_or_else(|| {
            AppError::Internal("xAI device authorization missing expires_in".into())
        })?,
    })
}

async fn poll_for_tokens(
    http: &reqwest::Client,
    endpoints: &XaiOAuthEndpoints,
    device: &DeviceCode,
) -> Result<LiveToken, AppError> {
    let deadline = now_ms() + (device.expires_in_seconds as i64) * 1000;
    let mut interval = Duration::from_secs(device.interval_seconds.max(1));
    tokio::time::sleep(interval).await;

    loop {
        if now_ms() >= deadline {
            return Err(AppError::Internal("xAI device flow timed out".into()));
        }

        let resp = http
            .post(&endpoints.token_url)
            .header("Accept", "application/json")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", endpoints.client_id.as_str()),
                ("device_code", device.device_code.as_str()),
            ])
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("xAI device token poll failed: {e}")))?;

        let status = resp.status();
        let body: TokenResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("xAI device token poll invalid JSON: {e}")))?;

        if status.is_success() {
            return credentials_from_token(&body, None);
        }

        match body.error.as_deref() {
            Some("authorization_pending") => {}
            Some("slow_down") => {
                interval += Duration::from_secs(SLOW_DOWN_INCREMENT_SECS);
            }
            Some("access_denied") | Some("authorization_denied") => {
                return Err(AppError::Auth("xAI device authorization was denied".into()));
            }
            Some("expired_token") => {
                return Err(AppError::Internal("xAI device code expired".into()));
            }
            _ => {
                return Err(AppError::Internal(format_oauth_error(
                    "device token polling",
                    status.as_u16(),
                    body.error.as_deref(),
                    body.error_description.as_deref(),
                )));
            }
        }

        let remaining = deadline.saturating_sub(now_ms());
        if remaining == 0 {
            return Err(AppError::Internal("xAI device flow timed out".into()));
        }
        let sleep_ms = interval.as_millis().min(remaining as u128) as u64;
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    }
}

fn credentials_from_token(
    body: &TokenResponse,
    previous_refresh: Option<&str>,
) -> Result<LiveToken, AppError> {
    let access = body
        .access_token
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| oauth_upstream("xAI token response missing access_token".into()))?
        .to_owned();
    let refresh = match body.refresh_token.as_deref().filter(|s| !s.is_empty()) {
        Some(r) => r.to_owned(),
        None => previous_refresh
            .filter(|s| !s.is_empty())
            .ok_or_else(|| oauth_upstream("xAI token response missing refresh_token".into()))?
            .to_owned(),
    };
    let lifetime = body
        .expires_in
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_TOKEN_LIFETIME_SECS);
    Ok(LiveToken {
        access,
        refresh,
        expires: now_ms() + lifetime * 1000 - REFRESH_SKEW_MS,
    })
}

fn required_string(value: Option<String>, field: &str) -> Result<String, AppError> {
    value
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Internal(format!("xAI OAuth response missing {field}")))
}

fn required_https(raw: Option<&str>, field: &str) -> Result<String, AppError> {
    let raw = raw
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Internal(format!("xAI OAuth response missing {field}")))?;
    if !raw.starts_with("https://") {
        return Err(AppError::Internal(format!(
            "untrusted {field} in xAI OAuth response"
        )));
    }
    Ok(raw.to_owned())
}

fn oauth_denied(status: u16, body: &TokenResponse) -> AppError {
    AppError::Upstream {
        status: Some(status),
        retryable: true,
        bad_key_hint: true,
        msg: format_oauth_error(
            "token refresh",
            status,
            body.error.as_deref(),
            body.error_description.as_deref(),
        ),
    }
}

fn oauth_upstream(msg: String) -> AppError {
    AppError::Upstream {
        status: Some(401),
        retryable: true,
        bad_key_hint: true,
        msg,
    }
}

fn format_oauth_error(
    action: &str,
    status: u16,
    error: Option<&str>,
    description: Option<&str>,
) -> String {
    let detail = [error, description]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(": ");
    if detail.is_empty() {
        format!("xAI OAuth {action} failed (HTTP {status})")
    } else {
        format!("xAI OAuth {action} failed (HTTP {status}): {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_https_verification_uri() {
        let err =
            required_https(Some("http://evil.example/login"), "verification_uri").unwrap_err();
        assert!(err.to_string().contains("untrusted"));
    }

    #[test]
    fn reuses_previous_refresh_when_omitted() {
        let body = TokenResponse {
            access_token: Some("new-access".into()),
            refresh_token: None,
            expires_in: Some(3600),
            error: None,
            error_description: None,
        };
        let live = credentials_from_token(&body, Some("old-refresh")).unwrap();
        assert_eq!(live.access, "new-access");
        assert_eq!(live.refresh, "old-refresh");
    }
}

//! Unified error type and OpenAI-format HTTP response conversion.
//!
//! All recoverable errors flow through [`AppError`] variants. The [`IntoResponse`]
//! implementation produces JSON bodies matching OpenAI's `{error:{message,type,code}}` schema.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Application-wide error, convertible to an OpenAI-format HTTP response.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Upstream error (status={status:?}): {msg}")]
    Upstream {
        status: Option<u16>,
        retryable: bool,
        bad_key_hint: bool,
        msg: String,
    },

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match self {
            AppError::Config(ref msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "config_error",
                msg.clone(),
            ),
            AppError::Auth(ref msg) => (StatusCode::UNAUTHORIZED, "auth_error", msg.clone()),
            AppError::NotFound(ref msg) => (StatusCode::NOT_FOUND, "not_found", msg.clone()),
            AppError::BadRequest(ref msg) => (StatusCode::BAD_REQUEST, "bad_request", msg.clone()),
            AppError::Upstream {
                status: upstream_status,
                msg,
                ..
            } => {
                let http_status = upstream_status
                    .filter(|s| (400..600).contains(s))
                    .map(StatusCode::from_u16)
                    .transpose()
                    .ok()
                    .flatten()
                    .unwrap_or(StatusCode::BAD_GATEWAY);
                (http_status, "upstream_error", msg)
            }
            AppError::Internal(ref msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                msg.clone(),
            ),
        };

        let body = json!({
            "error": {
                "message": message,
                "type": error_type,
                "code": status.as_u16()
            }
        });

        (status, axum::Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn into_response_json(err: AppError) -> (StatusCode, serde_json::Value) {
        let resp = err.into_response();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    fn upstream_err(status: Option<u16>, retryable: bool, bad_key_hint: bool) -> AppError {
        AppError::Upstream {
            status,
            retryable,
            bad_key_hint,
            msg: "test error".into(),
        }
    }

    #[tokio::test]
    async fn it_responds_500_with_error_json_for_config() {
        let (status, body) = into_response_json(AppError::Config("bad config".into())).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["type"], "config_error");
        assert_eq!(body["error"]["message"], "bad config");
        assert_eq!(body["error"]["code"], 500);
    }

    #[tokio::test]
    async fn it_responds_401_with_error_json_for_auth() {
        let (status, body) = into_response_json(AppError::Auth("invalid key".into())).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["type"], "auth_error");
        assert_eq!(body["error"]["message"], "invalid key");
        assert_eq!(body["error"]["code"], 401);
    }

    #[tokio::test]
    async fn it_responds_404_with_error_json_for_not_found() {
        let (status, body) = into_response_json(AppError::NotFound("model not found".into())).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["type"], "not_found");
        assert_eq!(body["error"]["message"], "model not found");
        assert_eq!(body["error"]["code"], 404);
    }

    #[tokio::test]
    async fn it_responds_400_with_error_json_for_bad_request() {
        let (status, body) =
            into_response_json(AppError::BadRequest("body too large".into())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["type"], "bad_request");
        assert_eq!(body["error"]["message"], "body too large");
        assert_eq!(body["error"]["code"], 400);
    }

    #[tokio::test]
    async fn it_responds_502_with_error_json_for_upstream() {
        let (status, body) = into_response_json(upstream_err(None, true, false)).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["type"], "upstream_error");
        assert_eq!(body["error"]["message"], "test error");
        assert_eq!(body["error"]["code"], 502);
    }

    #[tokio::test]
    async fn it_responds_500_with_error_json_for_internal() {
        let (status, body) = into_response_json(AppError::Internal("panic".into())).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["type"], "internal_error");
        assert_eq!(body["error"]["message"], "panic");
        assert_eq!(body["error"]["code"], 500);
    }

    #[tokio::test]
    async fn it_includes_content_type_json() {
        let resp = AppError::Auth("bad".into()).into_response();
        let content_type = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.contains("application/json"));
    }

    #[tokio::test]
    async fn upstream_status_429_maps_to_429() {
        let (status, body) = into_response_json(upstream_err(Some(429), true, true)).await;
        assert_eq!(status, StatusCode::from_u16(429).unwrap());
        assert_eq!(body["error"]["code"], 429);
    }

    #[tokio::test]
    async fn upstream_status_503_maps_to_503() {
        let (status, body) = into_response_json(upstream_err(Some(503), true, false)).await;
        assert_eq!(status, StatusCode::from_u16(503).unwrap());
        assert_eq!(body["error"]["code"], 503);
    }

    #[tokio::test]
    async fn upstream_status_200_maps_to_502() {
        let (status, body) = into_response_json(upstream_err(Some(200), false, false)).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], 502);
    }

    #[test]
    fn upstream_fields_are_transparent() {
        let err = AppError::Upstream {
            status: Some(429),
            retryable: true,
            bad_key_hint: true,
            msg: "rate limited".into(),
        };
        match err {
            AppError::Upstream {
                status,
                retryable,
                bad_key_hint,
                msg,
            } => {
                assert!(retryable);
                assert!(bad_key_hint);
                assert_eq!(status, Some(429));
                assert_eq!(msg, "rate limited");
            }
            _ => panic!("expected Upstream"),
        }

        let err = AppError::Upstream {
            status: Some(503),
            retryable: true,
            bad_key_hint: false,
            msg: "down".into(),
        };
        match err {
            AppError::Upstream {
                retryable,
                bad_key_hint,
                ..
            } => {
                assert!(retryable);
                assert!(!bad_key_hint);
            }
            _ => panic!("expected Upstream"),
        }
    }
}

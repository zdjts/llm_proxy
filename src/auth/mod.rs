//! Client-side API key authentication middleware.
//!
//! Validates the `Authorization: Bearer <key>` header against the configured
//! `client_keys` set. Rejected requests receive a 401 response in OpenAI error
//! format. Successful requests get an [`AuthedClient`] injected via axum
//! [`Extension`] for downstream handlers to consume.

use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::auth_store::AuthStore;
use crate::error::AppError;

/// Identifies the authenticated client whose key passed validation.
#[derive(Clone, Debug)]
pub struct AuthedClient {
    /// SHA-256 first 12 hex of the client API key.
    pub key_hash: String,
    /// Tenant identifier from the auth config, default `"default"`.
    pub tenant_id: String,
}

/// A single client key entry with tenant association.
#[derive(Clone, Debug, Deserialize)]
pub struct ClientKeyEntry {
    pub key: String,
    #[serde(default = "default_tenant")]
    pub tenant_id: String,
}

fn default_tenant() -> String {
    "default".into()
}

/// Set of valid client keys used by the auth middleware.
#[derive(Clone)]
pub struct AuthState {
    pub entries: Vec<ClientKeyEntry>,
    pub store: Option<std::sync::Arc<AuthStore>>,
}

/// axum middleware that rejects requests missing a valid `Authorization: Bearer` header.
///
/// On success, injects an [`AuthedClient`] extension into the request so
/// downstream handlers can identify the caller without re-parsing the header.
///
/// Attach to a router via [`axum::middleware::from_fn_with_state`].
pub async fn require_auth(
    State(state): State<AuthState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let entry = match auth_header {
        Some(key) => match &state.store {
            Some(store) => store.validate(key),
            None => state.entries.iter().find(|e| e.key == key).cloned(),
        },
        None => None,
    };

    match entry {
        Some(entry) => {
            let key = auth_header.unwrap_or_default();
            let key_hash = crate::db::compute_key_hash(key);
            request.extensions_mut().insert(AuthedClient {
                key_hash,
                tenant_id: entry.tenant_id.clone(),
            });
            Ok(next.run(request).await)
        }
        _ => {
            let err = AppError::Auth("Invalid or missing API key".into());
            Ok(err.into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::auth_store::AuthStore;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use axum::routing::get;
    use tower::util::ServiceExt;

    fn auth_state(keys: &[&str]) -> AuthState {
        AuthState {
            store: None,
            entries: keys
                .iter()
                .map(|s| ClientKeyEntry {
                    key: s.to_string(),
                    tenant_id: "default".into(),
                })
                .collect(),
        }
    }

    #[allow(dead_code)]
    fn store_auth_state(entries: &[&str]) -> AuthState {
        AuthState {
            store: Some(Arc::new(AuthStore::new(
                entries
                    .iter()
                    .map(|key| ClientKeyEntry {
                        key: (*key).to_owned(),
                        tenant_id: "default".into(),
                    })
                    .collect(),
            ))),
            entries: vec![ClientKeyEntry {
                key: "static-fallback-key".into(),
                tenant_id: "static".into(),
            }],
        }
    }

    fn test_app(state: AuthState) -> Router {
        Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(state, require_auth))
    }

    #[tokio::test]
    async fn store_is_the_only_authority_when_present() {
        let store = Arc::new(AuthStore::new(vec![ClientKeyEntry {
            key: "managed-key".into(),
            tenant_id: "managed".into(),
        }]));
        let app = test_app(AuthState {
            store: Some(Arc::clone(&store)),
            entries: vec![ClientKeyEntry {
                key: "managed-key".into(),
                tenant_id: "yaml".into(),
            }],
        });
        let req = Request::builder()
            .uri("/")
            .header(header::AUTHORIZATION, "Bearer managed-key")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let hash = crate::db::compute_key_hash("managed-key");
        store
            .update(
                &hash,
                crate::auth_store::UpdateKeyRequest {
                    enabled: Some(false),
                    label: None,
                    tenant_id: None,
                },
            )
            .unwrap();
        let req = Request::builder()
            .uri("/")
            .header(header::AUTHORIZATION, "Bearer managed-key")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let rotated = store
            .rotate(
                &hash,
                crate::auth_store::RotateKeyRequest {
                    new_key: "rotated-key".into(),
                },
            )
            .unwrap();
        assert_eq!(rotated.key_hash, crate::db::compute_key_hash("rotated-key"));
        let req = Request::builder()
            .uri("/")
            .header(header::AUTHORIZATION, "Bearer managed-key")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        store.remove(&rotated.key_hash).unwrap();
        let req = Request::builder()
            .uri("/")
            .header(header::AUTHORIZATION, "Bearer rotated-key")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    #[tokio::test]
    async fn it_passes_valid_key() {
        let app = test_app(auth_state(&["sk-test"]));
        let req = Request::builder()
            .uri("/")
            .header(header::AUTHORIZATION, "Bearer sk-test")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn it_rejects_missing_header() {
        let app = test_app(auth_state(&["sk-test"]));
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn it_rejects_invalid_key() {
        let app = test_app(auth_state(&["sk-test"]));
        let req = Request::builder()
            .uri("/")
            .header(header::AUTHORIZATION, "Bearer wrong-key")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn it_rejects_malformed_auth_header() {
        let app = test_app(auth_state(&["sk-test"]));
        let req = Request::builder()
            .uri("/")
            .header(header::AUTHORIZATION, "Basic sk-test")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn it_handles_multiple_client_keys() {
        let app = test_app(auth_state(&["key-a", "key-b", "key-c"]));
        let req = Request::builder()
            .uri("/")
            .header(header::AUTHORIZATION, "Bearer key-b")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn it_returns_openai_error_json_on_auth_failure() {
        let app = test_app(auth_state(&["sk-test"]));
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["error"]["type"], "auth_error");
        assert_eq!(json["error"]["code"], 401);
    }

    #[tokio::test]
    async fn it_injects_authed_client_extension() {
        use axum::extract::Extension;

        async fn check_ext(Extension(client): Extension<AuthedClient>) -> String {
            format!("key_hash={}", client.key_hash)
        }

        let state = auth_state(&["sk-test"]);
        let app = Router::new()
            .route("/check", get(check_ext))
            .layer(axum::middleware::from_fn_with_state(state, require_auth));

        let req = Request::builder()
            .uri("/check")
            .header(header::AUTHORIZATION, "Bearer sk-test")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8_lossy(&body_bytes);
        assert!(body_str.starts_with("key_hash="));
    }
}

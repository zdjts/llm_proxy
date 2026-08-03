//! Integration tests for the auth middleware.
//!
//! Exercises the full middleware chain: request ID → auth → handler.

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::http::{StatusCode, header};
use llm_proxy::auth::{AuthState, AuthedClient, require_auth};
use tower::util::ServiceExt;

fn auth_state(keys: &[&str]) -> AuthState {
    AuthState {
        store: None,
        entries: keys
            .iter()
            .map(|s| llm_proxy::auth::ClientKeyEntry {
                key: s.to_string(),
                tenant_id: "default".into(),
            })
            .collect(),
    }
}

fn test_app(state: AuthState) -> Router {
    Router::new()
        .route(
            "/",
            axum::routing::get(|| async { axum::http::StatusCode::OK }),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_auth))
}

#[tokio::test]
async fn correct_key_returns_200() {
    let app = test_app(auth_state(&["sk-test-key"]));
    let req = Request::builder()
        .uri("/")
        .header(header::AUTHORIZATION, "Bearer sk-test-key")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn wrong_key_returns_401() {
    let app = test_app(auth_state(&["sk-test-key"]));
    let req = Request::builder()
        .uri("/")
        .header(header::AUTHORIZATION, "Bearer wrong")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn missing_header_returns_401() {
    let app = test_app(auth_state(&["sk-test-key"]));
    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn non_bearer_scheme_returns_401() {
    let app = test_app(auth_state(&["sk-test-key"]));
    let req = Request::builder()
        .uri("/")
        .header(header::AUTHORIZATION, "Basic sk-test-key")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_injects_authed_client_extension() {
    let state = auth_state(&["sk-test-key"]);
    let app = Router::new()
        .route(
            "/check",
            axum::routing::get(
                |axum::Extension(c): axum::Extension<AuthedClient>| async move {
                    format!("hash:{}", c.key_hash)
                },
            ),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_auth));

    let req = Request::builder()
        .uri("/check")
        .header(header::AUTHORIZATION, "Bearer sk-test-key")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let body = String::from_utf8_lossy(&bytes);
    assert!(body.contains("hash:"));
}

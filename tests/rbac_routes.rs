//! Route-level RBAC integration tests (NEW-AUDIT-11).
//!
//! These tests build a real axum Router with `rbac_middleware` layered over
//! a protected handler, backed by a real SQLite pool with seeded roles.
//! They verify that requests are rejected or permitted at the HTTP level —
//! the critical gap identified in the foundation audit.

use std::sync::Arc;

use llm_proxy::db;
use llm_proxy::error::AppError;
use llm_proxy::rbac::middleware::{RbacState, rbac_middleware};
use llm_proxy::rbac::session::JwtService;
use llm_proxy::rbac::store;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use tower::util::ServiceExt;

/// A handler that requires authentication + permission. Returns 200 if the
/// user exists in request extensions and has the `audit.view` permission.
async fn protected_handler(
    request: axum::extract::Request,
) -> Result<axum::response::Json<serde_json::Value>, AppError> {
    let user = llm_proxy::rbac::middleware::require_permission(&request, "audit.view")?;
    Ok(axum::response::Json(serde_json::json!({
        "user_id": user.user_id,
        "email": user.email,
        "roles": user.roles,
    })))
}

async fn build_test_router(
    compat_mode: bool,
) -> (Router, sqlx::SqlitePool, Arc<JwtService>, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("rbac_route_test.db");
    let pool = db::connect(path.to_str().unwrap()).await.unwrap();

    store::seed_builtin_roles(&pool).await.unwrap();

    // Create a test user assigned the 'admin' role
    sqlx::query(
        "INSERT INTO user_account (id, email, name) VALUES ('test-user', 'test@example.com', 'Test User')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT OR IGNORE INTO user_role (user_id, role_id) VALUES ('test-user', 'admin')")
        .execute(&pool)
        .await
        .unwrap();

    let jwt = Arc::new(JwtService::new(
        b"test-secret-for-route-level-integration-test",
    ));
    let rbac_state = RbacState {
        pool: pool.clone(),
        jwt: Arc::clone(&jwt),
        compat_mode,
    };

    let app = Router::new()
        .route("/protected", get(protected_handler))
        .route_layer(from_fn_with_state(rbac_state, rbac_middleware));

    (app, pool, jwt, dir)
}

// ── Test: compat_mode = true → no token required ──────────────────────────

#[tokio::test]
async fn compat_mode_allows_unauthenticated() {
    let (app, _pool, _jwt, _dir) = build_test_router(true).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

// ── Test: compat_mode = false, no token → 401 ─────────────────────────────

#[tokio::test]
async fn no_token_without_compat_returns_401() {
    let (app, _pool, _jwt, _dir) = build_test_router(false).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Test: compat_mode = false, valid token → 200 ──────────────────────────

#[tokio::test]
async fn valid_token_without_compat_returns_200() {
    let (app, _pool, jwt, _dir) = build_test_router(false).await;

    let claims = llm_proxy::rbac::session::AccessClaims {
        sub: "test-user".into(),
        email: "test@example.com".into(),
        name: "Test User".into(),
        roles: vec!["admin".into()],
        permissions: vec!["audit.view".into()],
        teams: vec![],
        tenant_scope: None,
        iat: 0,
        exp: 0,
        jti: String::new(),
        iss: String::new(),
    };
    let token = jwt.issue_access(claims).unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

// ── Test: compat_mode = false, tampered token → 401 ───────────────────────

#[tokio::test]
async fn tampered_token_returns_401() {
    let (app, _pool, jwt, _dir) = build_test_router(false).await;

    let claims = llm_proxy::rbac::session::AccessClaims {
        sub: "test-user".into(),
        email: "test@example.com".into(),
        name: "Test User".into(),
        roles: vec!["admin".into()],
        permissions: vec!["audit.view".into()],
        teams: vec![],
        tenant_scope: None,
        iat: 0,
        exp: 0,
        jti: String::new(),
        iss: String::new(),
    };
    let token = jwt.issue_access(claims).unwrap();
    let mut parts: Vec<&str> = token.splitn(3, '.').collect();
    parts[2] = "tampered_signature";
    let tampered = parts.join(".");

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", format!("Bearer {tampered}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Test: valid token but insufficient permissions → 401 ───────────────────

#[tokio::test]
async fn valid_token_without_required_permission_returns_401() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("rbac_perm.db");
    let pool = db::connect(path.to_str().unwrap()).await.unwrap();

    store::seed_builtin_roles(&pool).await.unwrap();

    // Create a portal_user (only has portal.self, NOT audit.view)
    sqlx::query(
        "INSERT INTO user_account (id, email, name) VALUES ('portal-user', 'portal@example.com', 'Portal User')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO user_role (user_id, role_id) VALUES ('portal-user', 'portal_user')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let jwt = Arc::new(JwtService::new(b"test-secret-route-perm-test"));
    let rbac_state = RbacState {
        pool: pool.clone(),
        jwt: Arc::clone(&jwt),
        compat_mode: false,
    };

    let app = Router::new()
        .route("/protected", get(protected_handler))
        .route_layer(from_fn_with_state(rbac_state, rbac_middleware));

    let claims = llm_proxy::rbac::session::AccessClaims {
        sub: "portal-user".into(),
        email: "portal@example.com".into(),
        name: "Portal User".into(),
        roles: vec!["portal_user".into()],
        permissions: vec!["portal.self".into()],
        teams: vec![],
        tenant_scope: None,
        iat: 0,
        exp: 0,
        jti: String::new(),
        iss: String::new(),
    };
    let token = jwt.issue_access(claims).unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // The handler requires audit.view, but portal_user only has portal.self
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

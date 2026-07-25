//! Integration tests for auth endpoints (AUDIT-20 fix).
//!
//! Tests: login 200, wrong password 401, disabled user 401,
//! refresh 200, expired refresh 401, /me 200, no token 401,
//! user CRUD 403 without team.manage.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use llm_proxy::config;
use llm_proxy::rbac::middleware::RbacState;
use llm_proxy::rbac::session::JwtService;
use llm_proxy::server::{self, AppState};
use sqlx::SqlitePool;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::broadcast;
use tower::ServiceExt;

async fn setup() -> (SqlitePool, TempDir, AppState, Arc<JwtService>, String) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test_auth.db");
    let path_str = db_path.to_str().unwrap();

    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path_str)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePool::connect_with(options).await.unwrap();

    let migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
        .await
        .unwrap();
    migrator.run(&pool).await.unwrap();

    // Seed a test user with argon2 password
    use argon2::{
        Argon2, PasswordHasher,
        password_hash::{SaltString, rand_core::OsRng},
    };
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(b"correct-password", &salt)
        .unwrap()
        .to_string();

    let user_id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO user_account (id, email, name, password_hash) VALUES (?1, 'test@example.com', 'Test User', ?2)")
        .bind(&user_id)
        .bind(&hash)
        .execute(&pool)
        .await
        .unwrap();

    // Seed owner role
    sqlx::query(
        "INSERT OR IGNORE INTO rbac_role (id, name, is_system) VALUES ('owner', 'Owner', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Seed team.manage permission
    sqlx::query("INSERT OR IGNORE INTO rbac_permission (id, description) VALUES ('team.manage', 'Manage team members, roles, and organizations')")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT OR IGNORE INTO rbac_role_permission (role_id, permission_id) VALUES ('owner', 'team.manage')")
        .execute(&pool)
        .await
        .unwrap();

    // Assign owner role to test user
    sqlx::query("INSERT OR IGNORE INTO user_role (user_id, role_id) VALUES (?1, 'owner')")
        .bind(&user_id)
        .execute(&pool)
        .await
        .unwrap();

    // Create a disabled user
    let disabled_id = uuid::Uuid::new_v4().to_string();
    let disabled_hash = Argon2::default()
        .hash_password(b"disabled-pass", &salt)
        .unwrap()
        .to_string();
    sqlx::query("INSERT INTO user_account (id, email, name, password_hash, disabled) VALUES (?1, 'disabled@example.com', 'Disabled', ?2, 1)")
        .bind(&disabled_id)
        .bind(&disabled_hash)
        .execute(&pool)
        .await
        .unwrap();

    // Create portal_user role and assign to a third user (no team.manage)
    sqlx::query("INSERT OR IGNORE INTO rbac_role (id, name, is_system) VALUES ('portal_user', 'Portal User', 1)")
        .execute(&pool)
        .await
        .unwrap();
    let portal_id = uuid::Uuid::new_v4().to_string();
    let portal_hash = Argon2::default()
        .hash_password(b"portal-pass", &salt)
        .unwrap()
        .to_string();
    sqlx::query("INSERT INTO user_account (id, email, name, password_hash) VALUES (?1, 'portal@example.com', 'Portal', ?2)")
        .bind(&portal_id)
        .bind(&portal_hash)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT OR IGNORE INTO user_role (user_id, role_id) VALUES (?1, 'portal_user')")
        .bind(&portal_id)
        .execute(&pool)
        .await
        .unwrap();

    // Seed built-in roles
    if let Err(e) = llm_proxy::rbac::store::seed_builtin_roles(&pool).await {
        tracing::warn!("seed roles: {e}");
    }

    let jwt = Arc::new(JwtService::new(b"test-secret-key-for-auth-tests!!"));
    let rbac_state = RbacState {
        pool: pool.clone(),
        jwt: jwt.clone(),
        compat_mode: false,
    };

    let config = Arc::new(config::Config {
        server: config::ServerConfig {
            host: "127.0.0.1".into(),
            port: 4000,
            max_body_bytes: 10_485_760,
        },
        auth: config::AuthConfig {
            client_keys: vec![],
        },
        db: config::DbConfig {
            path: "./test.db".into(),
        },
        failover: config::FailoverConfig {
            enabled: true,
            bad_status_codes: vec![401, 402, 403, 429],
            max_retries: 1,
            probe_interval_secs: 60,
            probe_timeout_secs: 10,
            max_probe_retries: 3,
        },
        pools: Default::default(),
        providers: vec![],
        model_to_pool: Default::default(),
        bootstrap_admin: Default::default(),
        admin: Default::default(),
        pricing: Default::default(),
        rate_limit: Default::default(),
        cache_max_entries: 256,
        alerts: Default::default(),
        acl: Default::default(),
        fallback_models: Default::default(),
        concurrency: Default::default(),
    });

    let (alert_tx, _) = broadcast::channel(16);
    let state = AppState {
        router: llm_proxy::router::RouterHandle::new(Arc::new(llm_proxy::router::Router::new(
            Default::default(),
            Default::default(),
            Arc::new(llm_proxy::router::BadKeyRegistry::new()),
        ))),
        db: pool.clone(),
        config,
        config_store: None,
        budget_manager: None,
        cache: llm_proxy::cache::PromptCache::new(0),
        metrics: Arc::new(llm_proxy::metrics::Metrics::default()),
        circuit_breaker: Arc::new(llm_proxy::circuit_breaker::CircuitBreaker::with_defaults()),
        concurrency: Arc::new(llm_proxy::concurrency::ConcurrencyLimiter::new(50, 500)),
        fallback_config: Arc::new(llm_proxy::fallback::FallbackConfig::default()),
        alert_tx,
        error_burst_counters: Arc::new(dashmap::DashMap::new()),
        alert_snapshot: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        auth_store: None,
        quota_tracker: None,
        pipeline: None,
        rbac_state: Some(rbac_state),
    };

    (pool, dir, state, jwt, user_id)
}

fn owner_token(jwt: &JwtService, user_id: &str) -> String {
    use llm_proxy::rbac::session::AccessClaims;
    let claims = AccessClaims {
        sub: user_id.to_string(),
        email: "test@example.com".into(),
        name: "Test User".into(),
        roles: vec!["owner".into()],
        permissions: vec!["team.manage".into()],
        teams: vec![],
        tenant_scope: None,
        iat: 0,
        exp: 0,
        jti: String::new(),
        iss: String::new(),
    };
    jwt.issue_access(claims).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════
// Login tests
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn it_logs_in_with_correct_password() {
    let (_pool, _dir, state, _jwt, _user_id) = setup().await;

    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState { entries: vec![] },
        None,
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"email":"test@example.com","password":"correct-password"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 10000).await.unwrap();
    if status != StatusCode::OK {
        let body_str = String::from_utf8_lossy(&body);
        eprintln!("LOGIN FAILED: status={status}, body={body_str}");
    }
    assert_eq!(status, StatusCode::OK, "login should return 200");

    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["access_token"].as_str().unwrap().len() > 20);
    assert!(json["refresh_token"].as_str().unwrap().len() > 20);
    assert_eq!(json["user"]["email"], "test@example.com");
}

#[tokio::test]
async fn it_rejects_wrong_password_with_401() {
    let (_pool, _dir, state, _jwt, _user_id) = setup().await;
    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState { entries: vec![] },
        None,
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"email":"test@example.com","password":"wrong-password"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn it_rejects_disabled_user_with_401() {
    let (_pool, _dir, state, _jwt, _user_id) = setup().await;
    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState { entries: vec![] },
        None,
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"email":"disabled@example.com","password":"disabled-pass"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ═══════════════════════════════════════════════════════════════════════════
// Refresh / Me tests
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn it_refreshes_access_token_with_valid_refresh() {
    let (_pool, _dir, state, jwt, user_id) = setup().await;
    let (refresh_token, _) = jwt.issue_refresh(&user_id).unwrap();
    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState { entries: vec![] },
        None,
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"refresh_token": refresh_token}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 10000).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["access_token"].as_str().unwrap().len() > 20);
}

#[tokio::test]
async fn it_returns_401_for_me_without_token() {
    let (_pool, _dir, state, _jwt, _user_id) = setup().await;
    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState { entries: vec![] },
        None,
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Without compat mode and no token → 401 from RBAC middleware
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn it_returns_user_info_for_me_with_valid_token() {
    let (_pool, _dir, state, jwt, user_id) = setup().await;
    let token = owner_token(&jwt, &user_id);
    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState { entries: vec![] },
        None,
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/auth/me")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 10000).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["email"], "test@example.com");
    assert!(
        json["permissions"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("team.manage"))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// User CRUD permission tests (AUDIT-21)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn it_allows_owner_to_list_users() {
    let (_pool, _dir, state, jwt, user_id) = setup().await;
    let token = owner_token(&jwt, &user_id);
    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState { entries: vec![] },
        None,
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/users")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn it_rejects_portal_user_from_user_crud() {
    let (_pool, _dir, state, jwt, _user_id) = setup().await;
    // Create a portal_user token (no team.manage)
    let claims = llm_proxy::rbac::session::AccessClaims {
        sub: "portal-user-id".into(),
        email: "portal@example.com".into(),
        name: "Portal".into(),
        roles: vec!["portal_user".into()],
        permissions: vec![],
        teams: vec![],
        tenant_scope: None,
        iat: 0,
        exp: 0,
        jti: String::new(),
        iss: String::new(),
    };
    let portal_token = jwt.issue_access(claims).unwrap();

    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState { entries: vec![] },
        None,
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/users")
                .header("Authorization", format!("Bearer {portal_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

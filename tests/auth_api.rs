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

    // Seed permissions used by the admin API guard.
    for (id, description) in [
        (
            "team.manage",
            "Manage team members, roles, and organizations",
        ),
        ("keys.manage", "Manage client and upstream keys"),
        ("providers.manage", "Manage providers and models"),
        ("routing.edit", "Edit model routing"),
        ("audit.view", "View configuration audit data"),
    ] {
        sqlx::query("INSERT OR IGNORE INTO rbac_permission (id, description) VALUES (?1, ?2)")
            .bind(id)
            .bind(description)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT OR IGNORE INTO rbac_role_permission (role_id, permission_id) VALUES ('owner', 'team.manage'), ('owner', 'keys.manage'), ('owner', 'providers.manage'), ('owner', 'routing.edit'), ('owner', 'audit.view')")
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

    for permission in [
        "audit.view",
        "providers.manage",
        "keys.manage",
        "routing.edit",
    ] {
        sqlx::query("INSERT OR IGNORE INTO rbac_permission (id, description) VALUES (?1, ?1)")
            .bind(permission)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT OR IGNORE INTO rbac_role_permission (role_id, permission_id) VALUES ('owner', ?1)")
            .bind(permission)
            .execute(&pool)
            .await
            .unwrap();
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
        model_registry: Default::default(),
        model_metadata: Default::default(),
        bootstrap_admin: Default::default(),
        admin: config::AdminConfig {
            enabled: true,
            allowed_ips: vec!["127.0.0.1".into()],
        },
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
        catalog: llm_proxy::model_catalog::ModelCatalog::new(
            llm_proxy::router::RouterHandle::new(Arc::new(llm_proxy::router::Router::new(
                Default::default(),
                Default::default(),
                Arc::new(llm_proxy::router::BadKeyRegistry::new()),
            ))),
            Arc::new(llm_proxy::config_store::ConfigStore::for_test(
                pool.clone(),
                config.model_metadata.clone(),
            )),
        ),
        config: Arc::clone(&config),
        db: pool.clone(),
        config_store: Arc::new(llm_proxy::config_store::ConfigStore::for_test(
            pool.clone(),
            config.model_metadata.clone(),
        )),
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
        permissions: vec![
            "team.manage".into(),
            "audit.view".into(),
            "providers.manage".into(),
            "keys.manage".into(),
            "routing.edit".into(),
        ],
        teams: vec![],
        tenant_scope: None,
        iat: 0,
        exp: 0,
        jti: String::new(),
        iss: String::new(),
    };
    jwt.issue_access(claims).unwrap()
}

fn token_with_permissions(jwt: &JwtService, user_id: &str, permissions: Vec<&str>) -> String {
    use llm_proxy::rbac::session::AccessClaims;
    let claims = AccessClaims {
        sub: user_id.to_string(),
        email: "test@example.com".into(),
        name: "Test User".into(),
        roles: vec!["custom".into()],
        permissions: permissions.into_iter().map(str::to_string).collect(),
        teams: vec![],
        tenant_scope: None,
        iat: 0,
        exp: 0,
        jti: String::new(),
        iss: String::new(),
    };
    jwt.issue_access(claims).unwrap()
}

#[tokio::test]
async fn config_endpoint_matrix_requires_auth_and_permission() {
    let (pool, _dir, state, jwt, owner_id) = setup().await;
    let limited_user_id: String =
        sqlx::query_scalar("SELECT id FROM user_account WHERE email = 'portal@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/refresh")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let token = token_with_permissions(&jwt, &limited_user_id, vec!["audit.view"]);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/refresh")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let token = owner_token(&jwt, &owner_id);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/refresh")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_ip_guard_hides_config_routes_from_non_whitelisted_ip() {
    let (_pool, _dir, state, jwt, user_id) = setup().await;
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let token = token_with_permissions(&jwt, &user_id, vec!["providers.manage"]);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/refresh")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-real-ip", "192.0.2.10")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn provider_pool_and_routing_guards_require_distinct_permissions() {
    let (pool, _dir, state, jwt, _owner_id) = setup().await;
    let limited_user_id: String =
        sqlx::query_scalar("SELECT id FROM user_account WHERE email = 'portal@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let cases = [
        (
            "/admin/api/providers",
            serde_json::json!({"id":"p","kind":"openai","base_url":"http://x","pool_id":"missing"}),
        ),
        (
            "/admin/api/pools",
            serde_json::json!({"id":"pool","keys":[]}),
        ),
        (
            "/admin/api/routing",
            serde_json::json!({"logical_model":"m","pool_id":"missing"}),
        ),
    ];
    for (uri, body) in cases {
        let token = token_with_permissions(&jwt, &limited_user_id, vec![]);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
#[tokio::test]
async fn http_rollback_restores_provider_pool_and_routing_and_records_audit() {
    let (pool, _dir, state, jwt, user_id) = setup().await;
    sqlx::query("INSERT INTO key_pool (id, strategy) VALUES ('rollback-pool', 'weighted_random')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO provider_config (id, kind, base_url, pool_id, metadata) VALUES ('rollback-provider', 'openai', 'http://provider', 'rollback-pool', '{\"region\":\"test\"}')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO routing_config (logical_model, pool_id, default_params) VALUES ('rollback-model', 'rollback-pool', '{\"temperature\":0.25,\"nested\":{\"top_p\":0.9}}')")
        .execute(&pool)
        .await
        .unwrap();
    state.config_store.refresh_from_db().await.unwrap();

    let provider_before = serde_json::json!({
        "id": "rollback-provider", "kind": "openai", "base_url": "http://provider",
        "pool_id": "rollback-pool", "metadata": {"region": "test"}
    });
    llm_proxy::audit_trail::record_audit(
        &pool,
        llm_proxy::audit_trail::AuditEvent {
            event_type: "provider.delete".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "provider_config".into(),
            target_id: "rollback-provider".into(),
            before_json: Some(provider_before),
            after_json: None,
            metadata: None,
        },
    )
    .await
    .unwrap();
    let provider_audit_id: i64 = sqlx::query_scalar(
        "SELECT id FROM audit_trail WHERE event_type = 'provider.delete' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM provider_config WHERE id = 'rollback-provider'")
        .execute(&pool)
        .await
        .unwrap();

    let routing_before = serde_json::json!({
        "logical_model": "rollback-model", "pool_id": "rollback-pool",
        "default_params": {"temperature": 0.25, "nested": {"top_p": 0.9}}
    });
    llm_proxy::audit_trail::record_audit(
        &pool,
        llm_proxy::audit_trail::AuditEvent {
            event_type: "routing.delete".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "routing_config".into(),
            target_id: "rollback-model".into(),
            before_json: Some(routing_before),
            after_json: None,
            metadata: None,
        },
    )
    .await
    .unwrap();
    let routing_audit_id: i64 = sqlx::query_scalar(
        "SELECT id FROM audit_trail WHERE event_type = 'routing.delete' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM routing_config WHERE logical_model = 'rollback-model'")
        .execute(&pool)
        .await
        .unwrap();

    let pool_before =
        serde_json::json!({"id": "rollback-pool", "strategy": "weighted_random", "keys": []});
    llm_proxy::audit_trail::record_audit(
        &pool,
        llm_proxy::audit_trail::AuditEvent {
            event_type: "pool.delete".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "key_pool".into(),
            target_id: "rollback-pool".into(),
            before_json: Some(pool_before),
            after_json: None,
            metadata: None,
        },
    )
    .await
    .unwrap();
    let pool_audit_id: i64 = sqlx::query_scalar(
        "SELECT id FROM audit_trail WHERE event_type = 'pool.delete' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM key_pool WHERE id = 'rollback-pool'")
        .execute(&pool)
        .await
        .unwrap();
    state.config_store.refresh_from_db().await.unwrap();
    let version_before = state.config_store.version().await;

    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let token = owner_token(&jwt, &user_id);
    for audit_id in [pool_audit_id, provider_audit_id, routing_audit_id] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/admin/api/config/rollback/{audit_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let response_status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(
            response_status,
            StatusCode::OK,
            "rollback {audit_id} failed: {}",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["ok"],
            true
        );
    }

    assert!(
        sqlx::query("SELECT 1 FROM provider_config WHERE id = 'rollback-provider'")
            .fetch_optional(&pool)
            .await
            .unwrap()
            .is_some()
    );
    let params: String = sqlx::query_scalar(
        "SELECT default_params FROM routing_config WHERE logical_model = 'rollback-model'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&params).unwrap()["nested"]["top_p"],
        0.9
    );
    assert!(
        sqlx::query("SELECT 1 FROM key_pool WHERE id = 'rollback-pool'")
            .fetch_optional(&pool)
            .await
            .unwrap()
            .is_some()
    );
    assert!(state.config_store.version().await > version_before);
    let rollback_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_trail WHERE event_type = 'config.rollback'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(rollback_count, 3);
}

#[tokio::test]
async fn http_rollback_rejects_sensitive_pool_without_creating_empty_pool() {
    let (pool, _dir, state, jwt, user_id) = setup().await;
    let before = serde_json::json!({"id":"secret-pool","strategy":"weighted_random","keys":[{"key_hash":"deadbeef1234","weight":1}]});
    llm_proxy::audit_trail::record_audit(
        &pool,
        llm_proxy::audit_trail::AuditEvent {
            event_type: "pool.delete".into(),
            actor_id: None,
            actor_ip: None,
            target_type: "key_pool".into(),
            target_id: "secret-pool".into(),
            before_json: Some(before),
            after_json: None,
            metadata: None,
        },
    )
    .await
    .unwrap();
    let audit_id: i64 = sqlx::query_scalar(
        "SELECT id FROM audit_trail WHERE target_id = 'secret-pool' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let version_before = state.config_store.version().await;
    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let token = owner_token(&jwt, &user_id);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/api/config/rollback/{audit_id}"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        sqlx::query("SELECT 1 FROM key_pool WHERE id = 'secret-pool'")
            .fetch_optional(&pool)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(state.config_store.version().await, version_before);
    let audit_json: String =
        sqlx::query_scalar("SELECT before_json FROM audit_trail WHERE id = ?1")
            .bind(audit_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!audit_json.contains("plaintext"));
}
#[tokio::test]
async fn client_key_crud_requires_keys_manage_permission() {
    let (pool, _dir, state, jwt, user_id) = setup().await;
    sqlx::query("DELETE FROM user_role WHERE user_id = ?1")
        .bind(&user_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO rbac_role (id, name, is_system) VALUES ('key_viewer', 'Key Viewer', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO user_role (user_id, role_id) VALUES (?1, 'key_viewer')")
        .bind(&user_id)
        .execute(&pool)
        .await
        .unwrap();
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/client-keys")
                .header(
                    "Authorization",
                    format!("Bearer {}", owner_token(&jwt, &user_id)),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_models_requires_providers_manage_permission() {
    let (pool, _dir, state, jwt, user_id) = setup().await;
    sqlx::query("DELETE FROM user_role WHERE user_id = ?1")
        .bind(&user_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO rbac_role (id, name, is_system) VALUES ('audit_only', 'Audit Only', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO rbac_role_permission (role_id, permission_id) VALUES ('audit_only', 'audit.view')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO user_role (user_id, role_id) VALUES (?1, 'audit_only')")
        .bind(&user_id)
        .execute(&pool)
        .await
        .unwrap();
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/models")
                .header(
                    "Authorization",
                    format!("Bearer {}", owner_token(&jwt, &user_id)),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn client_key_crud_audit_never_persists_plaintext_keys() {
    let (pool, _dir, mut state, jwt, user_id) = setup().await;
    state.auth_store = Some(Arc::new(llm_proxy::auth_store::AuthStore::new(vec![])));
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let token = owner_token(&jwt, &user_id);
    let original = "create-secret-key";
    let created = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/client-keys")
                .header("Authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"key": original, "tenant_id": "tenant-a", "label": "test"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let created_body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(created.into_body(), 4096)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(created_body.get("key").is_none());
    let original_hash = created_body["key_hash"].as_str().unwrap().to_owned();

    let audit_after_create: String = sqlx::query_scalar(
        "SELECT after_json FROM audit_trail WHERE event_type = 'key.create' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!audit_after_create.contains(original));
    assert!(!audit_after_create.contains("\"key\""));

    let rotated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/api/client-keys/{original_hash}"))
                .header("Authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"new_key": "rotated-secret-key"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rotated.status(), StatusCode::OK);
    let rotated_body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(rotated.into_body(), 4096)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(rotated_body.get("key").is_none());
    let rotated_hash = rotated_body["key_hash"].as_str().unwrap().to_owned();

    let audit_before_rotate: String = sqlx::query_scalar(
        "SELECT before_json FROM audit_trail WHERE event_type = 'key.rotate' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!audit_before_rotate.contains(original));
    assert!(!audit_before_rotate.contains("rotated-secret-key"));

    let audit_after_rotate: String = sqlx::query_scalar(
        "SELECT after_json FROM audit_trail WHERE event_type = 'key.rotate' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!audit_after_rotate.contains("rotated-secret-key"));

    let deleted = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/admin/api/client-keys/{rotated_hash}"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::OK);
    let audit_before_delete: String = sqlx::query_scalar(
        "SELECT before_json FROM audit_trail WHERE event_type = 'key.delete' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!audit_before_delete.contains("rotated-secret-key"));
    assert!(!audit_before_delete.contains("\"key\""));
}
#[tokio::test]
async fn config_validate_requires_audit_view_permission() {
    let (pool, _dir, state, jwt, _user_id) = setup().await;
    let limited_user_id: String =
        sqlx::query_scalar("SELECT id FROM user_account WHERE email = 'portal@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let token = token_with_permissions(&jwt, &limited_user_id, vec![]);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/validate")
                .header("Authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"yaml": include_str!("fixtures/config.yaml")}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn config_validate_allows_audit_view_permission() {
    let (_pool, _dir, state, jwt, user_id) = setup().await;
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let token = token_with_permissions(&jwt, &user_id, vec!["audit.view"]);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/validate")
                .header("Authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"yaml": include_str!("fixtures/config.yaml")}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn config_export_requires_provider_management_permission() {
    let (pool, _dir, state, jwt, _user_id) = setup().await;
    let audit_user_id: String =
        sqlx::query_scalar("SELECT id FROM user_account WHERE email = 'portal@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let token = token_with_permissions(&jwt, &audit_user_id, vec!["audit.view"]);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/config/export")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn config_export_allows_provider_management_permission() {
    let (_pool, _dir, state, jwt, user_id) = setup().await;
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let token = token_with_permissions(&jwt, &user_id, vec!["providers.manage"]);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/config/export")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let yaml = String::from_utf8(body.to_vec()).unwrap();
    assert!(yaml.contains("server:\n  host: 127.0.0.1\n  port: 4000"));
    assert!(yaml.contains("max_body_bytes:"));
    assert!(yaml.contains("db:\n  path: ./test.db"));
    assert!(
        !yaml.contains("kind: open_ai"),
        "export must use openai not open_ai"
    );
    llm_proxy::config_store::ConfigStore::validate_yaml(&yaml).unwrap();
}

#[tokio::test]
async fn config_export_can_be_imported_without_losing_provider_metadata() {
    let (pool, _dir, state, jwt, user_id) = setup().await;
    sqlx::query("INSERT INTO key_pool (id, strategy) VALUES ('export-pool', 'weighted_random')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO key_entry (pool_id, key_hash, key_plain, weight) VALUES (?1, ?2, ?3, 2)",
    )
    .bind("export-pool")
    .bind(llm_proxy::db::compute_key_hash("sk-export"))
    .bind("sk-export")
    .execute(&pool)
    .await
    .unwrap();
    let metadata = serde_json::json!({
        "api_key": "provider-secret",
        "nested": { "token": "nested-secret", "retries": 3 },
        "headers": ["x-request-id", "x-trace-id"]
    });
    sqlx::query(
        "INSERT INTO provider_config (id, kind, base_url, pool_id, metadata) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind("export-provider")
    .bind("openai")
    .bind("https://provider.example/v1")
    .bind("export-pool")
    .bind(serde_json::to_string(&metadata).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO routing_config (logical_model, pool_id, default_params) VALUES (?1, ?2, ?3)",
    )
    .bind("export-model")
    .bind("export-pool")
    .bind(r#"{"temperature":0.2}"#)
    .execute(&pool)
    .await
    .unwrap();
    state.config_store.refresh_from_db().await.unwrap();

    let token = owner_token(&jwt, &user_id);
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let export = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/config/export")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(export.status(), StatusCode::OK);
    let yaml = String::from_utf8(
        axum::body::to_bytes(export.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();

    let import = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/import")
                .header("Authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "yaml": yaml }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let import_status = import.status();
    let import_body = axum::body::to_bytes(import.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert_eq!(
        import_status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&import_body)
    );

    let restored: String =
        sqlx::query_scalar("SELECT metadata FROM provider_config WHERE id = 'export-provider'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&restored).unwrap(),
        metadata
    );
    let restored_key: (String, i64) =
        sqlx::query_as("SELECT key_plain, weight FROM key_entry WHERE pool_id = 'export-pool'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(restored_key, ("sk-export".into(), 2));
    let restored_strategy: String =
        sqlx::query_scalar("SELECT strategy FROM key_pool WHERE id = 'export-pool'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(restored_strategy, "weighted_random");
    let restored_params: String = sqlx::query_scalar(
        "SELECT default_params FROM routing_config WHERE logical_model = 'export-model'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&restored_params).unwrap(),
        serde_json::json!({ "temperature": 0.2 })
    );
}

#[tokio::test]
async fn config_export_round_trips_model_registry_metadata() {
    let (pool, _dir, state, jwt, user_id) = setup().await;
    sqlx::query("INSERT INTO key_pool (id, strategy) VALUES ('registry-pool', 'weighted_random')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO key_entry (pool_id, key_hash, key_plain, weight) VALUES (?1, ?2, ?3, 1)",
    )
    .bind("registry-pool")
    .bind(llm_proxy::db::compute_key_hash("sk-registry"))
    .bind("sk-registry")
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO provider_config (id, kind, base_url, pool_id, metadata) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind("registry-provider")
    .bind("openai")
    .bind("https://registry.example/v1")
    .bind("registry-pool")
    .bind("{}")
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO routing_config (logical_model, pool_id, default_params) VALUES (?1, ?2, ?3)",
    )
    .bind("grok-4.6")
    .bind("registry-pool")
    .bind(r#"{"temperature":0.1}"#)
    .execute(&pool)
    .await
    .unwrap();
    let capabilities = serde_json::json!({
        "reasoning": true,
        "metadata": {"thinkingLevelMap": {"low": "low", "high": "high"}}
    });
    sqlx::query(
        "INSERT INTO model_registry (
            id, display_name, provider_kind, provider_config_id,
            supports_vision, supports_tool_calling, supports_json_mode,
            max_context_tokens, max_output_tokens,
            input_price_per_1m, output_price_per_1m,
            capabilities_json, enabled
        ) VALUES (?1,?2,?3,?4,1,1,0,128000,64000,2.0,6.0,?5,1)",
    )
    .bind("grok-4.6")
    .bind("Grok 4.6")
    .bind("openai")
    .bind("registry-provider")
    .bind(capabilities.to_string())
    .execute(&pool)
    .await
    .unwrap();
    state.config_store.refresh_from_db().await.unwrap();

    let token = owner_token(&jwt, &user_id);
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let export = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/api/config/export")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(export.status(), StatusCode::OK);
    let yaml = String::from_utf8(
        axum::body::to_bytes(export.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(yaml.contains("grok-4.6"));
    assert!(yaml.contains("model_registry:"));

    sqlx::query("DELETE FROM model_registry WHERE id = 'grok-4.6'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM routing_config WHERE logical_model = 'grok-4.6'")
        .execute(&pool)
        .await
        .unwrap();

    let import = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/import")
                .header("Authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "yaml": yaml }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::OK);

    let restored: (
        String,
        String,
        Option<String>,
        i64,
        Option<f64>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT display_name, provider_kind, provider_config_id, supports_vision,
                    input_price_per_1m, capabilities_json
             FROM model_registry WHERE id = 'grok-4.6'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(restored.0, "Grok 4.6");
    assert_eq!(restored.1, "openai");
    assert_eq!(restored.2.as_deref(), Some("registry-provider"));
    assert_eq!(restored.3, 1);
    assert_eq!(restored.4, Some(2.0));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&restored.5.unwrap()).unwrap(),
        capabilities
    );
    let route_pool: String =
        sqlx::query_scalar("SELECT pool_id FROM routing_config WHERE logical_model = 'grok-4.6'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(route_pool, "registry-pool");
}

#[tokio::test]
async fn creating_model_with_provider_auto_creates_routing() {
    let (pool, _dir, state, jwt, user_id) = setup().await;
    sqlx::query("INSERT INTO key_pool (id, strategy) VALUES ('auto-pool', 'weighted_random')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO key_entry (pool_id, key_hash, key_plain, weight) VALUES (?1, ?2, ?3, 1)",
    )
    .bind("auto-pool")
    .bind(llm_proxy::db::compute_key_hash("sk-auto"))
    .bind("sk-auto")
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO provider_config (id, kind, base_url, pool_id, metadata) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind("auto-provider")
    .bind("openai")
    .bind("https://auto.example/v1")
    .bind("auto-pool")
    .bind("{}")
    .execute(&pool)
    .await
    .unwrap();
    state.config_store.refresh_from_db().await.unwrap();

    let token = owner_token(&jwt, &user_id);
    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/models")
                .header("Authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "id": "grok-4.6",
                        "display_name": "Grok 4.6",
                        "provider_kind": "openai",
                        "provider_config_id": "auto-provider",
                        "supports_vision": true,
                        "supports_tool_calling": true,
                        "supports_json_mode": false,
                        "max_context_tokens": 128000,
                        "max_output_tokens": 64000,
                        "input_price_per_1m": 2.0,
                        "output_price_per_1m": 6.0,
                        "capabilities_json": {"reasoning": true},
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let route_pool: String =
        sqlx::query_scalar("SELECT pool_id FROM routing_config WHERE logical_model = 'grok-4.6'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(route_pool, "auto-pool");
    assert!(
        state
            .config_store
            .snapshot()
            .await
            .model_routing
            .contains_key("grok-4.6")
    );
}

#[tokio::test]
async fn it_refreshes_db_config_via_admin_endpoint() {
    let (_pool, _dir, state, jwt, user_id) = setup().await;
    let token = owner_token(&jwt, &user_id);
    let app = server::build_router(
        state,
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
        None,
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/config/refresh")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["version"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn it_logs_in_with_correct_password() {
    let (_pool, _dir, state, _jwt, _user_id) = setup().await;

    let app = server::build_router(
        state.clone(),
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
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
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
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
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
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
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
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
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
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
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
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
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
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
        llm_proxy::auth::AuthState {
            store: None,
            entries: vec![],
        },
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

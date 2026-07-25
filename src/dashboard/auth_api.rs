//! Authentication REST API — v4.1 Track K (T190-T192).
//! AUDIT-19 Fix: argon2id password hashing (no plain SHA-256).
//! AUDIT-21 Fix: require_permission in all user CRUD handlers.

use crate::error::AppError;
use crate::rbac::AuthenticatedUser;
use crate::server::AppState;
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

// ── argon2 helpers ────────────────────────────────────────────────────────

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};

fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(format!("argon2: {e}")))?
        .to_string();
    Ok(hash)
}

fn verify_password(password: &str, stored_hash: &str) -> bool {
    let parsed = match PasswordHash::new(stored_hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

// ── Request / Response types ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}
#[derive(Serialize)]
pub struct LoginResponse {
    pub user: UserInfo,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}
#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}
#[derive(Serialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub expires_in: u64,
}

#[derive(Serialize, Clone)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub name: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub teams: Vec<String>,
    pub avatar_url: Option<String>,
}

impl From<&AuthenticatedUser> for UserInfo {
    fn from(u: &AuthenticatedUser) -> Self {
        Self {
            id: u.user_id.clone(),
            email: u.email.clone(),
            name: u.name.clone(),
            roles: u.roles.clone(),
            permissions: u.permissions.clone(),
            teams: u.team_ids.clone(),
            avatar_url: None,
        }
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────

pub async fn auth_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let jwt = state
        .rbac_state
        .as_ref()
        .map(|rs| rs.jwt.clone())
        .ok_or_else(|| AppError::Internal("JWT service not configured".into()))?;

    #[derive(sqlx::FromRow)]
    #[allow(dead_code)]
    struct UserRow {
        id: String,
        email: String,
        name: String,
        password_hash: Option<String>,
        disabled: i32,
        avatar_url: Option<String>,
    }

    let user: UserRow = sqlx::query_as("SELECT id, email, name, password_hash, disabled, avatar_url FROM user_account WHERE email = ?1")
        .bind(&req.email).fetch_optional(&state.db).await
        .map_err(|e| AppError::Internal(format!("DB error: {e}")))?
        .ok_or_else(|| AppError::Auth("Invalid email or password".into()))?;

    if user.disabled != 0 {
        return Err(AppError::Auth("Account is disabled".into()));
    }

    match &user.password_hash {
        Some(stored) if verify_password(&req.password, stored) => {}
        _ => return Err(AppError::Auth("Invalid email or password".into())),
    }

    let auth_user = crate::rbac::store::load_user_permissions(&state.db, &user.id).await?;

    let claims = crate::rbac::session::AccessClaims {
        sub: user.id.clone(),
        email: user.email.clone(),
        name: user.name.clone(),
        roles: auth_user.roles.clone(),
        permissions: auth_user.permissions.clone(),
        teams: auth_user.team_ids.clone(),
        tenant_scope: auth_user.tenant_scope.clone(),
        iat: 0,
        exp: 0,
        jti: String::new(),
        iss: String::new(),
    };
    let access_token = jwt.issue_access(claims)?;
    let (refresh_token, _) = jwt.issue_refresh(&user.id)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let _ = sqlx::query("UPDATE user_account SET last_login = ?1 WHERE id = ?2")
        .bind(now)
        .bind(&user.id)
        .execute(&state.db)
        .await;

    Ok(Json(LoginResponse {
        user: UserInfo::from(&auth_user),
        access_token,
        refresh_token,
        expires_in: 900,
    }))
}

pub async fn auth_refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<RefreshResponse>, AppError> {
    let jwt = state
        .rbac_state
        .as_ref()
        .map(|rs| rs.jwt.clone())
        .ok_or_else(|| AppError::Internal("JWT service not configured".into()))?;
    let rc = jwt.verify_refresh(&req.refresh_token)?;
    let auth_user = crate::rbac::store::load_user_permissions(&state.db, &rc.sub).await?;
    let claims = crate::rbac::session::AccessClaims {
        sub: auth_user.user_id.clone(),
        email: auth_user.email.clone(),
        name: auth_user.name.clone(),
        roles: auth_user.roles.clone(),
        permissions: auth_user.permissions.clone(),
        teams: auth_user.team_ids.clone(),
        tenant_scope: auth_user.tenant_scope.clone(),
        iat: 0,
        exp: 0,
        jti: String::new(),
        iss: String::new(),
    };
    let access_token = jwt.issue_access(claims)?;
    Ok(Json(RefreshResponse {
        access_token,
        expires_in: 900,
    }))
}

pub async fn auth_me(
    State(_state): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> Result<Json<UserInfo>, AppError> {
    Ok(Json(UserInfo::from(&user)))
}

// ── User CRUD (AUDIT-21: require_permission on all write endpoints) ──────

#[derive(Serialize)]
pub struct UserListResponse {
    pub users: Vec<UserInfo>,
    pub total: usize,
}
#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub name: String,
    pub password: String,
    #[serde(default)]
    pub role_ids: Vec<String>,
}
#[derive(Deserialize)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub role_ids: Option<Vec<String>>,
}

fn require_team_manage(user: &AuthenticatedUser) -> Result<(), AppError> {
    if user.can("team.manage") {
        Ok(())
    } else {
        Err(AppError::Auth(
            "Permission denied: team.manage required".into(),
        ))
    }
}

pub async fn user_list(
    State(state): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> Result<Json<UserListResponse>, AppError> {
    require_team_manage(&user)?;
    #[derive(sqlx::FromRow)]
    #[allow(dead_code)]
    struct Row {
        id: String,
        email: String,
        name: String,
        disabled: i32,
        avatar_url: Option<String>,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, email, name, disabled, avatar_url FROM user_account ORDER BY email",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| AppError::Internal(format!("DB: {e}")))?;
    let mut users = Vec::new();
    for row in rows {
        let au = crate::rbac::store::load_user_permissions(&state.db, &row.id)
            .await
            .unwrap_or(AuthenticatedUser {
                user_id: row.id.clone(),
                email: row.email.clone(),
                name: row.name.clone(),
                roles: vec![],
                permissions: vec![],
                team_ids: vec![],
                tenant_scope: None,
            });
        users.push(UserInfo {
            id: row.id,
            email: row.email,
            name: row.name,
            roles: au.roles,
            permissions: au.permissions,
            teams: au.team_ids,
            avatar_url: row.avatar_url,
        });
    }
    let total = users.len();
    Ok(Json(UserListResponse { users, total }))
}

pub async fn user_create(
    State(state): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<UserInfo>, AppError> {
    require_team_manage(&user)?;
    let id = uuid::Uuid::new_v4().to_string();
    let hash = hash_password(&req.password)?;
    sqlx::query(
        "INSERT INTO user_account (id, email, name, password_hash) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(&id)
    .bind(&req.email)
    .bind(&req.name)
    .bind(&hash)
    .execute(&state.db)
    .await
    .map_err(|e| AppError::Internal(format!("DB: {e}")))?;
    for rid in &req.role_ids {
        let _ = sqlx::query("INSERT OR IGNORE INTO user_role (user_id, role_id) VALUES (?1, ?2)")
            .bind(&id)
            .bind(rid)
            .execute(&state.db)
            .await;
    }
    Ok(Json(UserInfo {
        id,
        email: req.email,
        name: req.name,
        roles: req.role_ids,
        permissions: vec![],
        teams: vec![],
        avatar_url: None,
    }))
}

pub async fn user_update(
    State(state): State<AppState>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<UserInfo>, AppError> {
    require_team_manage(&user)?;
    if let Some(ref n) = req.name {
        sqlx::query("UPDATE user_account SET name = ?1 WHERE id = ?2")
            .bind(n)
            .bind(&user_id)
            .execute(&state.db)
            .await
            .map_err(|e| AppError::Internal(format!("DB: {e}")))?;
    }
    if let Some(ref e) = req.email {
        sqlx::query("UPDATE user_account SET email = ?1 WHERE id = ?2")
            .bind(e)
            .bind(&user_id)
            .execute(&state.db)
            .await
            .map_err(|e| AppError::Internal(format!("DB: {e}")))?;
    }
    if let Some(ref p) = req.password {
        let h = hash_password(p)?;
        sqlx::query("UPDATE user_account SET password_hash = ?1 WHERE id = ?2")
            .bind(&h)
            .bind(&user_id)
            .execute(&state.db)
            .await
            .map_err(|e| AppError::Internal(format!("DB: {e}")))?;
    }
    if let Some(d) = req.disabled {
        sqlx::query("UPDATE user_account SET disabled = ?1 WHERE id = ?2")
            .bind(d as i32)
            .bind(&user_id)
            .execute(&state.db)
            .await
            .map_err(|e| AppError::Internal(format!("DB: {e}")))?;
    }
    if let Some(ref role_ids) = req.role_ids {
        sqlx::query("DELETE FROM user_role WHERE user_id = ?1")
            .bind(&user_id)
            .execute(&state.db)
            .await
            .map_err(|e| AppError::Internal(format!("DB: {e}")))?;
        for rid in role_ids {
            let _ =
                sqlx::query("INSERT OR IGNORE INTO user_role (user_id, role_id) VALUES (?1, ?2)")
                    .bind(&user_id)
                    .bind(rid)
                    .execute(&state.db)
                    .await;
        }
    }
    let au = crate::rbac::store::load_user_permissions(&state.db, &user_id).await?;
    Ok(Json(UserInfo::from(&au)))
}

pub async fn user_delete(
    State(state): State<AppState>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_team_manage(&user)?;
    sqlx::query("DELETE FROM user_role WHERE user_id = ?1")
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(format!("DB: {e}")))?;
    sqlx::query("DELETE FROM team_member WHERE user_id = ?1")
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(format!("DB: {e}")))?;
    sqlx::query("DELETE FROM user_account WHERE id = ?1")
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(format!("DB: {e}")))?;
    Ok(Json(serde_json::json!({"ok":true,"deleted":&user_id})))
}

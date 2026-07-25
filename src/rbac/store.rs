//! RBAC database store (Track D — T141/T143).
//!
//! Provides CRUD operations for roles, permissions, user assignments,
//! and refresh token management. All queries use parameterized SQL.

use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use sqlx::SqlitePool;

use crate::config::BootstrapAdminConfig;
use crate::error::AppError;
use crate::rbac::{AuthenticatedUser, BuiltInRole};

/// Load the full set of effective permissions for a user from the database.
pub async fn load_user_permissions(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<AuthenticatedUser, AppError> {
    // Get user account
    let user_row = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, disabled, tenant_scope FROM user_account WHERE id = ?1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(format!("DB error loading user: {e}")))?
    .ok_or_else(|| AppError::NotFound(format!("User '{user_id}' not found")))?;

    if user_row.disabled != 0 {
        return Err(AppError::Auth("Account is disabled".into()));
    }

    // Get assigned roles
    let roles: Vec<String> = sqlx::query_as::<_, RoleRef>(
        "SELECT r.id FROM rbac_role r \
         INNER JOIN user_role ur ON ur.role_id = r.id \
         WHERE ur.user_id = ?1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(format!("DB error loading roles: {e}")))?
    .into_iter()
    .map(|r| r.id)
    .collect();

    // Get permissions from all assigned roles
    let permissions: Vec<String> = if roles.is_empty() {
        vec![]
    } else {
        let placeholders: Vec<String> = roles
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let query = format!(
            "SELECT DISTINCT rp.permission_id FROM rbac_role_permission rp WHERE rp.role_id IN ({})",
            placeholders.join(",")
        );
        let mut q = sqlx::query_as::<_, PermissionRef>(&query);
        for role in &roles {
            q = q.bind(role);
        }
        q.fetch_all(pool)
            .await
            .map_err(|e| AppError::Internal(format!("DB error loading permissions: {e}")))?
            .into_iter()
            .map(|p| p.permission_id)
            .collect()
    };

    // Get team memberships
    let teams: Vec<String> =
        sqlx::query_as::<_, TeamRef>("SELECT team_id FROM team_member WHERE user_id = ?1")
            .bind(user_id)
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::Internal(format!("DB error loading teams: {e}")))?
            .into_iter()
            .map(|t| t.team_id)
            .collect();

    Ok(AuthenticatedUser {
        user_id: user_row.id,
        email: user_row.email,
        name: user_row.name,
        roles,
        permissions,
        team_ids: teams,
        tenant_scope: user_row.tenant_scope,
    })
}

/// Create the configured first administrator once, without overwriting an
/// existing account. The password is hashed before it reaches SQLite.
pub async fn bootstrap_admin(
    pool: &SqlitePool,
    config: &BootstrapAdminConfig,
) -> Result<(), AppError> {
    if !config.enabled {
        return Ok(());
    }
    if config.email.trim().is_empty() || config.name.trim().is_empty() || config.password.is_empty()
    {
        return Err(AppError::Config(
            "bootstrap_admin requires email, name, and password when enabled".into(),
        ));
    }
    let role_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM rbac_role WHERE id = ?1")
        .bind(&config.role)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Internal(format!("DB error checking bootstrap role: {e}")))?;
    if role_exists == 0 {
        return Err(AppError::Config(format!(
            "bootstrap_admin role '{}' does not exist",
            config.role
        )));
    }
    let existing = sqlx::query_scalar::<_, String>("SELECT id FROM user_account WHERE email = ?1")
        .bind(&config.email)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Internal(format!("DB error checking bootstrap admin: {e}")))?;
    if let Some(id) = existing {
        sqlx::query("INSERT OR IGNORE INTO user_role (user_id, role_id) VALUES (?1, ?2)")
            .bind(&id)
            .bind(&config.role)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(format!("DB error assigning bootstrap role: {e}")))?;
        return Ok(());
    }
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(config.password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(format!("argon2: {e}")))?
        .to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("DB error starting bootstrap transaction: {e}")))?;
    sqlx::query(
        "INSERT INTO user_account (id, email, name, password_hash) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(&id)
    .bind(&config.email)
    .bind(&config.name)
    .bind(&password_hash)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(format!("DB error creating bootstrap admin: {e}")))?;
    sqlx::query("INSERT INTO user_role (user_id, role_id) VALUES (?1, ?2)")
        .bind(&id)
        .bind(&config.role)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("DB error assigning bootstrap role: {e}")))?;
    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("DB error committing bootstrap admin: {e}")))?;
    tracing::info!(email = %config.email, role = %config.role, "Created bootstrap admin");
    Ok(())
}

pub async fn seed_builtin_roles(pool: &SqlitePool) -> Result<(), AppError> {
    // Insert all permission IDs
    for perm_id in crate::rbac::permissions::Permission::all_ids() {
        let desc = format!("Permission: {perm_id}");
        sqlx::query(
            "INSERT OR IGNORE INTO rbac_permission (id, description, category) VALUES (?1, ?2, 'general')",
        )
        .bind(&perm_id)
        .bind(&desc)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(format!("DB error seeding permission: {e}")))?;
    }

    // Insert built-in roles
    for role in BuiltInRole::all() {
        sqlx::query(
            "INSERT OR IGNORE INTO rbac_role (id, name, description, is_system) VALUES (?1, ?2, ?3, 1)",
        )
        .bind(role.id())
        .bind(role.name())
        .bind(format!("Built-in {} role", role.name()))
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(format!("DB error seeding role: {e}")))?;

        // Assign default permissions
        for perm_id in role.default_permissions() {
            sqlx::query(
                "INSERT OR IGNORE INTO rbac_role_permission (role_id, permission_id) VALUES (?1, ?2)",
            )
            .bind(role.id())
            .bind(&perm_id)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(format!("DB error seeding role-permission: {e}")))?;
        }
    }

    tracing::info!("Seeded {} built-in RBAC roles", BuiltInRole::all().len());
    Ok(())
}

/// Store a refresh token in the database.
pub async fn store_refresh_token(
    pool: &SqlitePool,
    user_id: &str,
    jti: &str,
    expires_at: u64,
) -> Result<(), AppError> {
    sqlx::query("INSERT INTO refresh_token (id, user_id, expires_at) VALUES (?1, ?2, ?3)")
        .bind(jti)
        .bind(user_id)
        .bind(expires_at as i64)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(format!("DB error storing refresh token: {e}")))?;
    Ok(())
}

/// Check if a refresh token is still valid (not revoked, not expired).
pub async fn is_refresh_token_valid(pool: &SqlitePool, jti: &str) -> Result<bool, AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let row = sqlx::query_as::<_, TokenRow>(
        "SELECT id FROM refresh_token WHERE id = ?1 AND revoked = 0 AND expires_at > ?2",
    )
    .bind(jti)
    .bind(now)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(format!("DB error checking refresh token: {e}")))?;

    Ok(row.is_some())
}

/// Revoke all refresh tokens for a user (e.g., on password change, logout all).
pub async fn revoke_user_tokens(pool: &SqlitePool, user_id: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE refresh_token SET revoked = 1 WHERE user_id = ?1")
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(format!("DB error revoking tokens: {e}")))?;
    Ok(())
}

/// Revoke a single refresh token.
pub async fn revoke_token(pool: &SqlitePool, jti: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE refresh_token SET revoked = 1 WHERE id = ?1")
        .bind(jti)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(format!("DB error revoking token: {e}")))?;
    Ok(())
}

// -- Row types for sqlx::query_as --

#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    email: String,
    name: String,
    disabled: i32,
    tenant_scope: Option<String>,
}

#[derive(sqlx::FromRow)]
struct RoleRef {
    id: String,
}

#[derive(sqlx::FromRow)]
struct PermissionRef {
    permission_id: String,
}

#[derive(sqlx::FromRow)]
struct TeamRef {
    team_id: String,
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    #[allow(dead_code)]
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;
    use tempfile::TempDir;

    async fn setup_rbac_pool() -> (SqlitePool, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test_rbac.db");
        let pool = crate::db::connect(path.to_str().unwrap()).await.unwrap();
        (pool, dir)
    }

    #[tokio::test]
    async fn it_seeds_builtin_roles_idempotently() {
        let (pool, _dir) = setup_rbac_pool().await;

        seed_builtin_roles(&pool).await.unwrap();
        // Second call should be idempotent
        seed_builtin_roles(&pool).await.unwrap();

        let count: i64 = sqlx::query("SELECT COUNT(*) FROM rbac_role WHERE is_system = 1")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, 6, "Should have 6 built-in roles");
    }

    #[tokio::test]
    async fn it_stores_and_validates_refresh_token() {
        let (pool, _dir) = setup_rbac_pool().await;

        // Insert a test user first
        sqlx::query(
            "INSERT INTO user_account (id, email, name) VALUES ('u1', 'test@t.com', 'Test')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let jti = "test-jti-1";
        let future = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;

        store_refresh_token(&pool, "u1", jti, future).await.unwrap();

        let valid = is_refresh_token_valid(&pool, jti).await.unwrap();
        assert!(valid);

        // Revoke
        revoke_token(&pool, jti).await.unwrap();
        let valid = is_refresh_token_valid(&pool, jti).await.unwrap();
        assert!(!valid);
    }

    #[tokio::test]
    async fn it_loads_user_permissions() {
        let (pool, _dir) = setup_rbac_pool().await;
        seed_builtin_roles(&pool).await.unwrap();

        // Insert a user and assign admin role
        sqlx::query(
            "INSERT INTO user_account (id, email, name) VALUES ('u2', 'admin@t.com', 'Admin User')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("INSERT OR IGNORE INTO user_role (user_id, role_id) VALUES ('u2', 'admin')")
            .execute(&pool)
            .await
            .unwrap();

        let user = load_user_permissions(&pool, "u2").await.unwrap();
        assert_eq!(user.user_id, "u2");
        assert_eq!(user.email, "admin@t.com");
        assert!(user.roles.contains(&"admin".to_string()));
        assert!(user.can("providers.manage"));
        assert!(user.can("keys.manage"));
        // Admin should NOT have team.manage (only Owner does)
        assert!(!user.can("team.manage"));
    }

    #[tokio::test]
    async fn it_loads_owner_with_all_permissions() {
        let (pool, _dir) = setup_rbac_pool().await;
        seed_builtin_roles(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO user_account (id, email, name) VALUES ('u3', 'owner@t.com', 'Owner')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("INSERT OR IGNORE INTO user_role (user_id, role_id) VALUES ('u3', 'owner')")
            .execute(&pool)
            .await
            .unwrap();

        let user = load_user_permissions(&pool, "u3").await.unwrap();
        assert!(user.can("providers.manage"));
        assert!(user.can("team.manage"));
        assert!(user.can("audit.view"));
    }

    #[tokio::test]
    async fn it_rejects_disabled_user() {
        let (pool, _dir) = setup_rbac_pool().await;

        sqlx::query(
            "INSERT INTO user_account (id, email, name, disabled) VALUES ('u4', 'dis@t.com', 'Disabled', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = load_user_permissions(&pool, "u4").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("disabled"));
    }

    #[tokio::test]
    async fn it_revokes_all_user_tokens() {
        let (pool, _dir) = setup_rbac_pool().await;

        sqlx::query("INSERT INTO user_account (id, email, name) VALUES ('u5', 'u5@t.com', 'U5')")
            .execute(&pool)
            .await
            .unwrap();

        let future = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;

        store_refresh_token(&pool, "u5", "jti-a", future)
            .await
            .unwrap();
        store_refresh_token(&pool, "u5", "jti-b", future)
            .await
            .unwrap();

        revoke_user_tokens(&pool, "u5").await.unwrap();

        assert!(!is_refresh_token_valid(&pool, "jti-a").await.unwrap());
        assert!(!is_refresh_token_valid(&pool, "jti-b").await.unwrap());
    }

    #[tokio::test]
    async fn it_handles_portal_user_tenant_scope() {
        let (pool, _dir) = setup_rbac_pool().await;
        seed_builtin_roles(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO user_account (id, email, name, tenant_scope) VALUES ('u6', 'portal@t.com', 'Portal User', 'tenant-x')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT OR IGNORE INTO user_role (user_id, role_id) VALUES ('u6', 'portal_user')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let user = load_user_permissions(&pool, "u6").await.unwrap();
        assert_eq!(user.tenant_scope, Some("tenant-x".to_string()));
        assert!(user.can_access_tenant("tenant-x"));
        assert!(!user.can_access_tenant("tenant-y"));
        assert!(user.can("portal.self"));
        assert!(!user.can("providers.manage"));
    }
}

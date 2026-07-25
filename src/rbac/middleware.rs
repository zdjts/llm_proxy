//! RBAC authorization middleware (Track D — T143).
//!
//! Provides axum middleware that:
//! 1. Extracts and validates the JWT Bearer token from the Authorization header
//! 2. Loads the user's effective permissions from the database
//! 3. Injects `AuthenticatedUser` into request extensions
//! 4. Optionally checks a required permission (via `RequirePermission` layer)

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sqlx::SqlitePool;

use crate::error::AppError;
use crate::rbac::AuthenticatedUser;
use crate::rbac::session::JwtService;

/// Application state needed by the RBAC middleware.
#[derive(Clone)]
pub struct RbacState {
    pub pool: SqlitePool,
    pub jwt: Arc<JwtService>,
    /// When true, unauthenticated requests get "compatibility mode" —
    /// granted full Owner access. This allows gradual migration.
    pub compat_mode: bool,
}

/// Extract the Bearer token, validate it, and inject `AuthenticatedUser`.
pub async fn rbac_middleware(
    State(state): State<RbacState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match auth_header {
        Some(token) => {
            match state.jwt.verify_access(token) {
                Ok(claims) => {
                    // Check if token is still valid in DB context
                    match crate::rbac::store::load_user_permissions(&state.pool, &claims.sub).await
                    {
                        Ok(user) => {
                            request.extensions_mut().insert(user);
                            Ok(next.run(request).await)
                        }
                        Err(e) => {
                            tracing::warn!(
                                user_id = %claims.sub,
                                error = %e,
                                "RBAC: user lookup failed"
                            );
                            Err((StatusCode::UNAUTHORIZED, e.to_string()).into_response())
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "RBAC: JWT verification failed");
                    Err((StatusCode::UNAUTHORIZED, e.to_string()).into_response())
                }
            }
        }
        None if state.compat_mode => {
            // Compatibility mode: create a synthetic Owner user
            let compat_user = AuthenticatedUser {
                user_id: "__compat_admin__".into(),
                email: "admin@localhost".into(),
                name: "Default Admin (compat mode)".into(),
                roles: vec!["owner".into()],
                permissions: crate::rbac::permissions::Permission::all_ids(),
                team_ids: vec![],
                tenant_scope: None,
            };
            request.extensions_mut().insert(compat_user);
            Ok(next.run(request).await)
        }
        None => Err((StatusCode::UNAUTHORIZED, "Missing Authorization header").into_response()),
    }
}

/// Helper to extract the authenticated user from request extensions.
///
/// Returns `AppError::Auth` if no user is present.
pub fn extract_user<B>(request: &Request<B>) -> Result<&AuthenticatedUser, AppError> {
    request
        .extensions()
        .get::<AuthenticatedUser>()
        .ok_or_else(|| AppError::Auth("Authentication required".into()))
}

/// Helper to check a specific permission on the authenticated user.
pub fn require_permission<'a, B>(
    request: &'a Request<B>,
    permission: &str,
) -> Result<&'a AuthenticatedUser, AppError> {
    let user = request
        .extensions()
        .get::<AuthenticatedUser>()
        .ok_or_else(|| AppError::Auth("Authentication required".into()))?;

    if !user.can(permission) {
        return Err(AppError::Auth(format!(
            "Permission '{permission}' required"
        )));
    }
    Ok(user)
}

/// Helper to check tenant-scoped access.
pub fn require_tenant_access<'a, B>(
    request: &'a Request<B>,
    tenant_id: &str,
) -> Result<&'a AuthenticatedUser, AppError> {
    let user = request
        .extensions()
        .get::<AuthenticatedUser>()
        .ok_or_else(|| AppError::Auth("Authentication required".into()))?;

    if !user.can_access_tenant(tenant_id) {
        return Err(AppError::Auth(format!(
            "Access to tenant '{tenant_id}' denied"
        )));
    }
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;

    #[test]
    fn it_extracts_user_from_extensions() {
        let user = AuthenticatedUser {
            user_id: "u1".into(),
            email: "test@t.com".into(),
            name: "Test".into(),
            roles: vec![],
            permissions: vec!["test.perm".into()],
            team_ids: vec![],
            tenant_scope: None,
        };

        let mut request = Request::builder().uri("/test").body(Body::empty()).unwrap();
        request.extensions_mut().insert(user);

        let extracted = extract_user(&request).unwrap();
        assert_eq!(extracted.user_id, "u1");
    }

    #[test]
    fn it_rejects_missing_user() {
        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let result = extract_user(&request);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Authentication required")
        );
    }

    #[test]
    fn it_checks_permission() {
        let user = AuthenticatedUser {
            user_id: "u1".into(),
            email: "t@t.com".into(),
            name: "T".into(),
            roles: vec![],
            permissions: vec!["providers.manage".into()],
            team_ids: vec![],
            tenant_scope: None,
        };

        let mut request = Request::builder().uri("/test").body(Body::empty()).unwrap();
        request.extensions_mut().insert(user);

        assert!(require_permission(&request, "providers.manage").is_ok());
        let result = require_permission(&request, "keys.manage");
        assert!(result.is_err());
    }

    #[test]
    fn it_checks_tenant_scope() {
        let user = AuthenticatedUser {
            user_id: "u1".into(),
            email: "t@t.com".into(),
            name: "T".into(),
            roles: vec![],
            permissions: vec![],
            team_ids: vec![],
            tenant_scope: Some("tenant-a".into()),
        };

        let mut request = Request::builder().uri("/test").body(Body::empty()).unwrap();
        request.extensions_mut().insert(user);

        assert!(require_tenant_access(&request, "tenant-a").is_ok());
        let result = require_tenant_access(&request, "tenant-b");
        assert!(result.is_err());
    }
}

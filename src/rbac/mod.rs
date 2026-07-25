//! Role-Based Access Control (RBAC) — Track D, v3.0.
//!
//! Implements a lightweight RBAC system with:
//! - Six built-in roles (Owner, Admin, Operator, Billing, ReadOnly, PortalUser)
//! - Custom roles support
//! - Fine-grained permissions at "page.operation" granularity
//! - JWT-based session management
//!
//! ## Design
//!
//! - All permissions are checked at the middleware/handler level, never in frontend.
//! - Frontend only uses the permission manifest to show/hide UI elements.
//! - Scope isolation: every tenant-scoped query must include `tenant_id` filter.

pub mod middleware;
pub mod permissions;
pub mod session;
pub mod store;

use serde::{Deserialize, Serialize};

/// A permission identifier at the "resource.action" level.
///
/// Examples: `providers.manage`, `keys.rotate`, `routing.edit`, `billing.export`.
pub type PermissionId = String;

/// The six built-in roles that ship with v3.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInRole {
    Owner,
    Admin,
    Operator,
    Billing,
    ReadOnly,
    PortalUser,
}

impl BuiltInRole {
    pub fn id(&self) -> &str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Operator => "operator",
            Self::Billing => "billing",
            Self::ReadOnly => "readonly",
            Self::PortalUser => "portal_user",
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Owner => "Owner",
            Self::Admin => "Admin",
            Self::Operator => "Operator",
            Self::Billing => "Billing",
            Self::ReadOnly => "Read Only",
            Self::PortalUser => "Portal User",
        }
    }

    /// All built-in roles in descending privilege order.
    pub fn all() -> [Self; 6] {
        [
            Self::Owner,
            Self::Admin,
            Self::Operator,
            Self::Billing,
            Self::ReadOnly,
            Self::PortalUser,
        ]
    }

    /// Permissions granted to this built-in role.
    pub fn default_permissions(&self) -> Vec<PermissionId> {
        use permissions::Permission;
        match self {
            Self::Owner => {
                // Owner gets ALL permissions
                Permission::all_ids()
            }
            Self::Admin => vec![
                Permission::PROVIDERS_MANAGE,
                Permission::KEYS_MANAGE,
                Permission::ROUTING_EDIT,
                Permission::ROUTING_SIMULATE,
                Permission::PIPELINE_EDIT,
                Permission::PIPELINE_VIEW,
                Permission::QUOTAS_MANAGE,
                Permission::BILLING_VIEW,
                Permission::BILLING_EXPORT,
                Permission::ALERTS_MANAGE,
                Permission::ALERTS_VIEW,
                Permission::AUDIT_VIEW,
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
            Self::Operator => vec![
                Permission::KEYS_MANAGE,
                Permission::ROUTING_SIMULATE,
                Permission::PIPELINE_VIEW,
                Permission::ALERTS_VIEW,
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
            Self::Billing => vec![
                Permission::BILLING_VIEW,
                Permission::BILLING_EXPORT,
                Permission::QUOTAS_MANAGE,
                Permission::ALERTS_VIEW,
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
            Self::ReadOnly => vec![
                Permission::ROUTING_SIMULATE,
                Permission::PIPELINE_VIEW,
                Permission::BILLING_VIEW,
                Permission::ALERTS_VIEW,
                Permission::AUDIT_VIEW,
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
            Self::PortalUser => vec![Permission::PORTAL_SELF]
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

/// An authenticated user with their effective permissions.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub email: String,
    pub name: String,
    pub roles: Vec<String>,
    pub permissions: Vec<PermissionId>,
    pub team_ids: Vec<String>,
    /// The tenant scope for portal users (non-portal users get `None` = all tenants).
    pub tenant_scope: Option<String>,
}

impl AuthenticatedUser {
    /// Check whether this user holds a specific permission.
    pub fn can(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }

    /// Check whether this user can access data for the given tenant.
    pub fn can_access_tenant(&self, tenant_id: &str) -> bool {
        self.tenant_scope
            .as_ref()
            .is_none_or(|scope| scope == tenant_id)
    }
}

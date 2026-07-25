//! RBAC permission definitions (Track D — T141).
//!
//! Each permission is at the `resource.action` level. The full permission matrix
//! is documented in `docs/BRIEF-v3.0-fullstack-upgrade.md` §7.
//!
//! All new Admin API handlers must declare their required permission via
//! `#[permission("resource.action")]` or check via `user.can(...)`.

/// Permission constants for all v3.0 operations.
///
/// Naming convention: `<resource>.<action>` where:
/// - `manage` = full CRUD + configuration
/// - `edit` = modify existing resources
/// - `view` = read-only access
/// - `export` = data export
/// - `rotate` = key rotation
/// - `simulate` = read-only simulation (no real impact)
/// - `self` = self-service portal access
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Permission;

impl Permission {
    // -- Provider management --
    pub const PROVIDERS_MANAGE: &str = "providers.manage";

    // -- Key management --
    pub const KEYS_MANAGE: &str = "keys.manage";
    pub const KEYS_ROTATE: &str = "keys.rotate";

    // -- Routing --
    pub const ROUTING_EDIT: &str = "routing.edit";
    pub const ROUTING_SIMULATE: &str = "routing.simulate";

    // -- Pipeline --
    pub const PIPELINE_EDIT: &str = "pipeline.edit";
    pub const PIPELINE_VIEW: &str = "pipeline.view";

    // -- Quotas & Billing --
    pub const QUOTAS_MANAGE: &str = "quotas.manage";
    pub const BILLING_VIEW: &str = "billing.view";
    pub const BILLING_EXPORT: &str = "billing.export";

    // -- Alerts --
    pub const ALERTS_MANAGE: &str = "alerts.manage";
    pub const ALERTS_VIEW: &str = "alerts.view";

    // -- Team & RBAC --
    pub const TEAM_MANAGE: &str = "team.manage";

    // -- Audit --
    pub const AUDIT_VIEW: &str = "audit.view";

    // -- Self-service portal --
    pub const PORTAL_SELF: &str = "portal.self";

    /// All known permission identifiers.
    pub fn all_ids() -> Vec<String> {
        vec![
            Self::PROVIDERS_MANAGE,
            Self::KEYS_MANAGE,
            Self::KEYS_ROTATE,
            Self::ROUTING_EDIT,
            Self::ROUTING_SIMULATE,
            Self::PIPELINE_EDIT,
            Self::PIPELINE_VIEW,
            Self::QUOTAS_MANAGE,
            Self::BILLING_VIEW,
            Self::BILLING_EXPORT,
            Self::ALERTS_MANAGE,
            Self::ALERTS_VIEW,
            Self::TEAM_MANAGE,
            Self::AUDIT_VIEW,
            Self::PORTAL_SELF,
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    /// Permissions grouped by category for the role editor UI.
    pub fn by_category() -> Vec<(&'static str, Vec<&'static str>)> {
        vec![
            ("Providers", vec![Self::PROVIDERS_MANAGE]),
            ("Keys", vec![Self::KEYS_MANAGE, Self::KEYS_ROTATE]),
            ("Routing", vec![Self::ROUTING_EDIT, Self::ROUTING_SIMULATE]),
            ("Pipeline", vec![Self::PIPELINE_EDIT, Self::PIPELINE_VIEW]),
            (
                "Quotas & Billing",
                vec![
                    Self::QUOTAS_MANAGE,
                    Self::BILLING_VIEW,
                    Self::BILLING_EXPORT,
                ],
            ),
            ("Alerts", vec![Self::ALERTS_MANAGE, Self::ALERTS_VIEW]),
            ("Team", vec![Self::TEAM_MANAGE]),
            ("Audit", vec![Self::AUDIT_VIEW]),
            ("Portal", vec![Self::PORTAL_SELF]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_permissions_have_unique_ids() {
        let ids = Permission::all_ids();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "permission IDs must be unique");
    }

    #[test]
    fn owner_has_all_permissions() {
        let owner_perms = crate::rbac::BuiltInRole::Owner.default_permissions();
        let all = Permission::all_ids();
        assert_eq!(
            owner_perms.len(),
            all.len(),
            "Owner must have all permissions"
        );
        for p in &all {
            assert!(owner_perms.contains(p), "Owner missing permission: {p}");
        }
    }

    #[test]
    fn portal_user_only_has_self() {
        let perms = crate::rbac::BuiltInRole::PortalUser.default_permissions();
        assert_eq!(perms.len(), 1);
        assert_eq!(perms[0], Permission::PORTAL_SELF);
    }

    #[test]
    fn readonly_cannot_edit() {
        let perms = crate::rbac::BuiltInRole::ReadOnly.default_permissions();
        assert!(!perms.contains(&Permission::KEYS_MANAGE.to_string()));
        assert!(!perms.contains(&Permission::ROUTING_EDIT.to_string()));
        assert!(!perms.contains(&Permission::PIPELINE_EDIT.to_string()));
        assert!(!perms.contains(&Permission::ALERTS_MANAGE.to_string()));
    }

    #[test]
    fn each_builtin_role_has_permissions() {
        for role in crate::rbac::BuiltInRole::all() {
            let perms = role.default_permissions();
            assert!(
                !perms.is_empty(),
                "Role {:?} must have at least one permission",
                role
            );
        }
    }

    #[test]
    fn permission_categories_cover_all() {
        let all_ids: std::collections::HashSet<String> =
            Permission::all_ids().into_iter().collect();
        let mut cat_ids = std::collections::HashSet::new();
        for (_, perms) in Permission::by_category() {
            for p in perms {
                cat_ids.insert(p.to_string());
            }
        }
        assert_eq!(all_ids, cat_ids, "categories must cover all permissions");
    }
}

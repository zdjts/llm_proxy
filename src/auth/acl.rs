//! Per-tenant Model Access Control (ACL) middleware.
//!
//! Configuration: `acl` block in `config.yaml` with per-tenant model allowlists.
//! When enabled, requests for models not listed in the tenant's allowlist receive 404.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use dashmap::DashMap;
use serde::Deserialize;

use crate::auth::AuthedClient;
use crate::error::AppError;

/// Per-tenant ACL configuration (deserialized from `acl` YAML key).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AclConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub tenants: HashMap<String, TenantAcl>,
}

/// A tenant's model allowlist or blocklist.
#[derive(Debug, Clone, Deserialize)]
pub struct TenantAcl {
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub default: AclDefault,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AclDefault {
    Allow,
    #[default]
    Deny,
}

impl TenantAcl {
    /// Check if a model is allowed for this tenant.
    pub fn allows(&self, model: &str) -> bool {
        match self.default {
            AclDefault::Allow => !self.models.iter().any(|m| m == model),
            AclDefault::Deny => self.models.iter().any(|m| m == model),
        }
    }
}

/// Shared ACL state, updated on config reload.
#[derive(Clone)]
pub struct AclState {
    pub inner: Arc<DashMap<String, TenantAcl>>,
    pub enabled: bool,
}

impl AclState {
    pub fn from_config(config: &AclConfig) -> Self {
        let map = DashMap::new();
        for (tenant, acl) in &config.tenants {
            map.insert(tenant.clone(), acl.clone());
        }
        Self {
            inner: Arc::new(map),
            enabled: config.enabled,
        }
    }

    pub fn allows(&self, tenant: &str, model: &str) -> bool {
        if !self.enabled {
            return true;
        }
        match self.inner.get(tenant) {
            Some(acl) => acl.allows(model),
            None => false,
        }
    }
}

/// Axum middleware that enforces tenant ACL on `/v1/chat/completions`.
pub async fn acl_middleware(
    State(acl_state): State<Arc<AclState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    if !request.uri().path().ends_with("/chat/completions") {
        return Ok(next.run(request).await);
    }

    if !acl_state.enabled {
        return Ok(next.run(request).await);
    }

    // Extract tenant from extensions before consuming request body
    let tenant = request
        .extensions()
        .get::<AuthedClient>()
        .map(|c| c.tenant_id.clone());

    // Decompose the request to get parts (headers, extensions) and body separately
    let (parts, body) = request.into_parts();
    let body_bytes = match axum::body::to_bytes(body, 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return Ok(AppError::BadRequest("body too large".into()).into_response());
        }
    };

    let model: Option<String> = serde_json::from_slice::<serde_json::Value>(&body_bytes)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str().map(String::from)));

    if let (Some(model), Some(tenant)) = (model, tenant)
        && !acl_state.allows(&tenant, &model)
    {
        return Ok(AppError::NotFound(format!(
            "model '{model}' not available for tenant '{tenant}'"
        ))
        .into_response());
    }

    let new_request = Request::from_parts(parts, Body::from(body_bytes));
    Ok(next.run(new_request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_default_allows_listed_models() {
        let acl = TenantAcl {
            models: vec!["gpt-4o".into(), "deepseek-chat".into()],
            default: AclDefault::Deny,
        };
        assert!(acl.allows("gpt-4o"));
        assert!(acl.allows("deepseek-chat"));
        assert!(!acl.allows("gemini-pro"));
    }

    #[test]
    fn allow_default_denies_listed_models() {
        let acl = TenantAcl {
            models: vec!["gpt-4o".into()],
            default: AclDefault::Allow,
        };
        assert!(!acl.allows("gpt-4o"));
        assert!(acl.allows("any-other-model"));
    }

    #[test]
    fn acl_state_disabled_allows_all() {
        let state = AclState {
            inner: Arc::new(DashMap::new()),
            enabled: false,
        };
        assert!(state.allows("any-tenant", "any-model"));
    }

    #[test]
    fn acl_state_enabled_denies_unknown_tenant() {
        let state = AclState {
            inner: Arc::new(DashMap::new()),
            enabled: true,
        };
        assert!(!state.allows("unknown-tenant", "gpt-4o"));
    }
}

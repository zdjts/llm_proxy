//! Dynamic client-key management store (Module A1 — v2.0).
//!
//! Replaces the static `AuthState` with a DashMap-backed `AuthStore` that
//! supports runtime CRUD, key rotation, and optional persistence back to
//! the config file.  Exposed via `/admin/api/client-keys`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::auth::ClientKeyEntry;
use crate::db;
use crate::error::AppError;
use axum::response::IntoResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientKeyRecord {
    pub key: String,
    pub tenant_id: String,
    pub key_hash: String,
    pub created_at: i64,
    pub enabled: bool,
    pub label: String,
}

impl From<ClientKeyEntry> for ClientKeyRecord {
    fn from(e: ClientKeyEntry) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let key_hash = db::compute_key_hash(&e.key);
        Self {
            key: e.key,
            tenant_id: e.tenant_id,
            key_hash,
            created_at: now,
            enabled: true,
            label: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateKeyRequest {
    pub key: String,
    #[serde(default = "default_tenant")]
    pub tenant_id: String,
    #[serde(default)]
    pub label: String,
}

fn default_tenant() -> String {
    "default".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateKeyRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub tenant_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RotateKeyRequest {
    pub new_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyListResponse {
    pub keys: Vec<ClientKeyRecord>,
    pub total: usize,
}

pub struct AuthStore {
    keys: DashMap<String, ClientKeyRecord>,
}

impl AuthStore {
    pub fn new(initial: Vec<ClientKeyEntry>) -> Self {
        let store = Self {
            keys: DashMap::new(),
        };
        for entry in initial {
            let record: ClientKeyRecord = entry.into();
            store.keys.insert(record.key.clone(), record);
        }
        store
    }

    pub fn validate(&self, raw_key: &str) -> Option<ClientKeyEntry> {
        self.keys.get(raw_key).and_then(|r| {
            if r.enabled {
                Some(ClientKeyEntry {
                    key: r.key.clone(),
                    tenant_id: r.tenant_id.clone(),
                })
            } else {
                None
            }
        })
    }

    pub fn add(&self, req: CreateKeyRequest) -> Result<ClientKeyRecord, AppError> {
        if req.key.is_empty() {
            return Err(AppError::BadRequest("key must not be empty".into()));
        }
        if self.keys.contains_key(&req.key) {
            return Err(AppError::BadRequest("key already exists".into()));
        }
        let entry = ClientKeyEntry {
            key: req.key.clone(),
            tenant_id: req.tenant_id,
        };
        let record: ClientKeyRecord = entry.into();
        let mut record = record;
        record.label = req.label;
        self.keys.insert(req.key, record.clone());
        Ok(record)
    }

    pub fn update(
        &self,
        key_hash: &str,
        req: UpdateKeyRequest,
    ) -> Result<ClientKeyRecord, AppError> {
        let raw_key = self
            .keys
            .iter()
            .find(|e| e.value().key_hash == key_hash)
            .map(|e| e.key().clone());

        let raw_key = match raw_key {
            Some(k) => k,
            None => return Err(AppError::NotFound(format!("key '{key_hash}' not found"))),
        };

        let mut record = self
            .keys
            .get_mut(&raw_key)
            .ok_or_else(|| AppError::NotFound(format!("key '{key_hash}' not found")))?;

        if let Some(enabled) = req.enabled {
            record.enabled = enabled;
        }
        if let Some(label) = req.label {
            record.label = label;
        }
        if let Some(tenant_id) = req.tenant_id
            && !tenant_id.is_empty()
        {
            record.tenant_id = tenant_id;
        }
        Ok(record.clone())
    }

    pub fn rotate(
        &self,
        key_hash: &str,
        req: RotateKeyRequest,
    ) -> Result<ClientKeyRecord, AppError> {
        let old_key = self
            .keys
            .iter()
            .find(|e| e.value().key_hash == key_hash)
            .map(|e| e.key().clone());

        let old_key = match old_key {
            Some(k) => k,
            None => return Err(AppError::NotFound(format!("key '{key_hash}' not found"))),
        };

        if self.keys.contains_key(&req.new_key) {
            return Err(AppError::BadRequest(
                "new key already exists in the store".into(),
            ));
        }

        let mut old_record = self
            .keys
            .remove(&old_key)
            .ok_or_else(|| AppError::NotFound(format!("key '{key_hash}' not found")))?
            .1;

        let new_hash = db::compute_key_hash(&req.new_key);
        old_record.key = req.new_key.clone();
        old_record.key_hash = new_hash;
        old_record.created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        self.keys.insert(req.new_key, old_record.clone());
        Ok(old_record)
    }

    pub fn remove(&self, key_hash: &str) -> Result<ClientKeyRecord, AppError> {
        let key = self
            .keys
            .iter()
            .find(|e| e.value().key_hash == key_hash)
            .map(|e| e.key().clone());

        match key {
            Some(k) => self
                .keys
                .remove(&k)
                .map(|(_, v)| v)
                .ok_or_else(|| AppError::NotFound(format!("key '{key_hash}' not found"))),
            None => Err(AppError::NotFound(format!("key '{key_hash}' not found"))),
        }
    }

    pub fn list(&self) -> Vec<ClientKeyRecord> {
        let mut keys: Vec<ClientKeyRecord> = self.keys.iter().map(|e| e.value().clone()).collect();
        keys.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        keys
    }

    pub fn get_by_hash(&self, key_hash: &str) -> Option<ClientKeyRecord> {
        self.keys
            .iter()
            .find(|e| e.value().key_hash == key_hash)
            .map(|e| e.value().clone())
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

#[derive(Clone)]
pub struct AuthStoreState {
    pub store: Arc<AuthStore>,
}

impl AuthStoreState {
    pub fn new(initial: Vec<ClientKeyEntry>) -> Self {
        Self {
            store: Arc::new(AuthStore::new(initial)),
        }
    }
}

pub async fn auth_store_middleware(
    axum::extract::State(state): axum::extract::State<AuthStoreState>,
    mut request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, axum::response::Response> {
    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match auth_header {
        Some(key) => match state.store.validate(key) {
            Some(entry) => {
                let key_hash = db::compute_key_hash(key);
                request.extensions_mut().insert(crate::auth::AuthedClient {
                    key_hash,
                    tenant_id: entry.tenant_id,
                });
                Ok(next.run(request).await)
            }
            None => {
                let err = AppError::Auth("Invalid or missing API key".into());
                Ok(err.into_response())
            }
        },
        None => {
            let err = AppError::Auth("Invalid or missing API key".into());
            Ok(err.into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> AuthStore {
        AuthStore::new(vec![ClientKeyEntry {
            key: "sk-test".into(),
            tenant_id: "default".into(),
        }])
    }

    #[test]
    fn it_validates_existing_key() {
        let store = test_store();
        let entry = store.validate("sk-test").unwrap();
        assert_eq!(entry.key, "sk-test");
    }

    #[test]
    fn it_rejects_unknown_key() {
        let store = test_store();
        assert!(store.validate("sk-unknown").is_none());
    }

    #[test]
    fn it_adds_new_key() {
        let store = test_store();
        let record = store
            .add(CreateKeyRequest {
                key: "sk-new".into(),
                tenant_id: "org-b".into(),
                label: "test".into(),
            })
            .unwrap();
        assert_eq!(record.key, "sk-new");
        assert_eq!(record.tenant_id, "org-b");
        assert!(store.validate("sk-new").is_some());
    }

    #[test]
    fn it_rejects_duplicate_key() {
        let store = test_store();
        let err = store
            .add(CreateKeyRequest {
                key: "sk-test".into(),
                tenant_id: "default".into(),
                label: String::new(),
            })
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn it_rotates_key() {
        let store = test_store();
        let old_hash = db::compute_key_hash("sk-test");
        let record = store
            .rotate(
                &old_hash,
                RotateKeyRequest {
                    new_key: "sk-rotated".into(),
                },
            )
            .unwrap();
        assert_eq!(record.key, "sk-rotated");
        assert!(store.validate("sk-test").is_none());
        assert!(store.validate("sk-rotated").is_some());
    }

    #[test]
    fn it_removes_key() {
        let store = test_store();
        let hash = db::compute_key_hash("sk-test");
        let record = store.remove(&hash).unwrap();
        assert_eq!(record.key, "sk-test");
        assert!(store.validate("sk-test").is_none());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn it_disables_key() {
        let store = test_store();
        let hash = db::compute_key_hash("sk-test");
        store
            .update(
                &hash,
                UpdateKeyRequest {
                    enabled: Some(false),
                    label: None,
                    tenant_id: None,
                },
            )
            .unwrap();
        assert!(store.validate("sk-test").is_none());
    }
}

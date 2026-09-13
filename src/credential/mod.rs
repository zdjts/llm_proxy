//! Upstream credential types: static API keys and refreshable OAuth tokens.
//!
//! Providers still receive a [`KeyEntry`] whose `key` field is the current
//! Bearer token. This module keeps OAuth access tokens fresh and persists
//! rotations without changing the stable `identity_hash`.

pub mod xai;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::config::KeyEntry;
use crate::error::AppError;

/// Live OAuth token material keyed by stable identity hash.
#[derive(Debug, Clone)]
pub struct LiveToken {
    pub access: String,
    pub refresh: String,
    pub expires: i64,
}

/// Endpoints for the xAI device-code / refresh client.
#[derive(Debug, Clone)]
pub struct XaiOAuthEndpoints {
    pub token_url: String,
    pub device_code_url: String,
    pub client_id: String,
}

impl Default for XaiOAuthEndpoints {
    fn default() -> Self {
        Self {
            token_url: xai::TOKEN_URL.into(),
            device_code_url: xai::DEVICE_CODE_URL.into(),
            client_id: xai::CLIENT_ID.into(),
        }
    }
}

/// Process-wide OAuth token cache + optional SQLite persistence.
pub struct CredentialRuntime {
    tokens: DashMap<String, LiveToken>,
    locks: DashMap<String, Arc<Mutex<()>>>,
    http: reqwest::Client,
    db: Option<SqlitePool>,
    xai: XaiOAuthEndpoints,
}

impl CredentialRuntime {
    pub fn new(db: Option<SqlitePool>) -> Self {
        Self {
            tokens: DashMap::new(),
            locks: DashMap::new(),
            http: reqwest::Client::new(),
            db,
            xai: XaiOAuthEndpoints::default(),
        }
    }

    pub fn with_xai(db: Option<SqlitePool>, xai: XaiOAuthEndpoints) -> Self {
        Self {
            tokens: DashMap::new(),
            locks: DashMap::new(),
            http: reqwest::Client::new(),
            db,
            xai,
        }
    }

    /// Return a KeyEntry whose `key` is a currently-valid Bearer token.
    pub async fn ensure_fresh(&self, entry: &KeyEntry) -> Result<KeyEntry, AppError> {
        if !entry.is_oauth() {
            return Ok(entry.clone());
        }
        let identity = entry.identity_hash();
        let lock = self
            .locks
            .entry(identity.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;

        if let Some(live) = self.tokens.get(&identity)
            && !token_expired(live.expires)
        {
            return Ok(apply_live(entry, live.value()));
        }

        if let Some(expires) = entry.expires
            && !token_expired(expires)
        {
            if let Some(refresh) = entry.refresh.as_ref() {
                self.tokens.insert(
                    identity,
                    LiveToken {
                        access: entry.key.clone(),
                        refresh: refresh.clone(),
                        expires,
                    },
                );
            }
            return Ok(entry.clone());
        }

        let refresh = entry
            .refresh
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::Upstream {
                status: Some(401),
                retryable: true,
                bad_key_hint: true,
                msg: "oauth credential is missing a refresh token".into(),
            })?;

        let issuer = entry.issuer.as_deref().unwrap_or_default();
        let live = match issuer {
            "xai" => xai::refresh_token(&self.http, &self.xai, refresh).await?,
            other => {
                return Err(AppError::Config(format!(
                    "unsupported oauth issuer '{other}'"
                )));
            }
        };

        self.tokens.insert(identity.clone(), live.clone());
        self.persist(&identity, &live).await;
        Ok(apply_live(entry, &live))
    }

    async fn persist(&self, identity_hash: &str, live: &LiveToken) {
        let Some(db) = self.db.as_ref() else {
            return;
        };
        if let Err(e) = sqlx::query(
            "UPDATE key_entry SET key_plain = ?1, refresh_token = ?2, expires_at = ?3, \
             updated_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) WHERE key_hash = ?4",
        )
        .bind(&live.access)
        .bind(&live.refresh)
        .bind(live.expires)
        .bind(identity_hash)
        .execute(db)
        .await
        {
            tracing::warn!(%identity_hash, error = %e, "failed to persist oauth token rotation");
        }
    }
}

fn apply_live(entry: &KeyEntry, live: &LiveToken) -> KeyEntry {
    let mut out = entry.clone();
    if out.identity.as_deref().filter(|s| !s.is_empty()).is_none() {
        out.identity = Some(entry.identity_hash());
    }
    out.key = live.access.clone();
    out.refresh = Some(live.refresh.clone());
    out.expires = Some(live.expires);
    out
}

fn token_expired(expires_ms: i64) -> bool {
    now_ms() >= expires_ms
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Device-code prompt shown to the operator during interactive login.
#[derive(Debug, Clone)]
pub struct DeviceCodePrompt {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in_seconds: u64,
}

#[cfg(test)]
mod tests {
    use crate::config::KeyEntry;

    #[test]
    fn identity_hash_is_stable_when_access_token_rotates() {
        let a = KeyEntry::oauth("access-1", "refresh-stable", "xai", 1, Some(1));
        let mut b = a.clone();
        b.key = "access-2".into();
        assert_eq!(a.identity_hash(), b.identity_hash());
        assert_ne!(a.identity_hash(), crate::db::compute_key_hash(&a.key));
    }

    #[test]
    fn api_key_identity_is_the_key_itself() {
        let k = KeyEntry::api_key("sk-abc", 1);
        assert_eq!(k.identity_hash(), crate::db::compute_key_hash("sk-abc"));
        assert!(!k.is_oauth());
    }
}

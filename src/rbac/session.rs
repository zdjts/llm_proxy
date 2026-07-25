//! JWT session management for RBAC (Track D — T142).
//!
//! Issues short-lived access tokens (15 min) and long-lived refresh tokens (7 days).
//! Refresh tokens are stored in SQLite for revocation support.

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::error::AppError;

type HmacSha256 = Hmac<Sha256>;

/// Trait for claims that have an expiration field.
pub trait HasExpiration {
    fn expiration_secs(&self) -> u64;
}

/// JWT claims for an access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessClaims {
    /// Subject: user UUID
    pub sub: String,
    /// User email
    pub email: String,
    /// User display name
    pub name: String,
    /// Assigned role IDs
    pub roles: Vec<String>,
    /// Effective permissions
    pub permissions: Vec<String>,
    /// Team memberships (team IDs)
    pub teams: Vec<String>,
    /// Tenant scope (None = all tenants, Some("tenant-x") = scoped)
    pub tenant_scope: Option<String>,
    /// Issued at (epoch seconds)
    pub iat: u64,
    /// Expiration (epoch seconds)
    pub exp: u64,
    /// Unique token ID
    pub jti: String,
    /// Issuer
    pub iss: String,
}

impl HasExpiration for AccessClaims {
    fn expiration_secs(&self) -> u64 {
        self.exp
    }
}

/// JWT claims for a refresh token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshClaims {
    pub sub: String,
    pub jti: String,
    pub iat: u64,
    pub exp: u64,
    pub iss: String,
}

impl HasExpiration for RefreshClaims {
    fn expiration_secs(&self) -> u64 {
        self.exp
    }
}

/// JWT signing and verification service.
pub struct JwtService {
    secret: Vec<u8>,
    issuer: String,
}

impl JwtService {
    /// Create a new JWT service with the given signing secret.
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            secret: secret.into(),
            issuer: "llm_proxy".to_string(),
        }
    }

    /// Issue an access token valid for 15 minutes.
    pub fn issue_access(&self, claims: AccessClaims) -> Result<String, AppError> {
        let mut c = claims;
        let now = now_secs();
        c.iat = now;
        c.exp = now + 900; // 15 minutes
        c.iss = self.issuer.clone();
        c.jti = uuid::Uuid::new_v4().to_string();
        self.encode(&c)
    }

    /// Issue a refresh token valid for 7 days.
    pub fn issue_refresh(&self, sub: &str) -> Result<(String, RefreshClaims), AppError> {
        let now = now_secs();
        let claims = RefreshClaims {
            sub: sub.to_string(),
            jti: uuid::Uuid::new_v4().to_string(),
            iat: now,
            exp: now + 604_800, // 7 days
            iss: self.issuer.clone(),
        };
        let token = self.encode(&claims)?;
        Ok((token, claims))
    }

    /// Verify and decode an access token.
    pub fn verify_access(&self, token: &str) -> Result<AccessClaims, AppError> {
        self.decode::<AccessClaims>(token)
    }

    /// Verify and decode a refresh token.
    pub fn verify_refresh(&self, token: &str) -> Result<RefreshClaims, AppError> {
        self.decode::<RefreshClaims>(token)
    }

    pub(crate) fn encode<T: Serialize>(&self, claims: &T) -> Result<String, AppError> {
        let header = base64_url_encode(
            &serde_json::to_vec(&serde_json::json!({"alg":"HS256","typ":"JWT"}))
                .map_err(|e| AppError::Internal(format!("JWT header encode: {e}")))?,
        );
        let payload = base64_url_encode(
            &serde_json::to_vec(claims)
                .map_err(|e| AppError::Internal(format!("JWT payload encode: {e}")))?,
        );
        let signing_input = format!("{header}.{payload}");

        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .map_err(|e| AppError::Internal(format!("JWT HMAC init: {e}")))?;
        mac.update(signing_input.as_bytes());
        let signature = base64_url_encode(&mac.finalize().into_bytes());

        Ok(format!("{signing_input}.{signature}"))
    }

    fn decode<T: for<'de> Deserialize<'de> + HasExpiration>(
        &self,
        token: &str,
    ) -> Result<T, AppError> {
        let parts: Vec<&str> = token.splitn(3, '.').collect();
        if parts.len() != 3 {
            return Err(AppError::Auth("Invalid token format".into()));
        }

        let header_b64 = parts[0];
        let payload_b64 = parts[1];
        let signature_b64 = parts[2];

        // ── Constant-time signature verification (Fix 3) ──
        // HMAC verify_slice uses subtle crate for constant-time comparison.
        let signing_input = format!("{header_b64}.{payload_b64}");
        let decoded_sig = base64_url_decode(signature_b64)
            .map_err(|e| AppError::Auth(format!("Invalid token signature encoding: {e}")))?;

        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .map_err(|e| AppError::Internal(format!("JWT HMAC init: {e}")))?;
        mac.update(signing_input.as_bytes());

        mac.verify_slice(&decoded_sig)
            .map_err(|_| AppError::Auth("Invalid token signature".into()))?;

        // Decode payload (signature has been verified, MAC is consumed)
        let payload_bytes = base64_url_decode(payload_b64)
            .map_err(|e| AppError::Auth(format!("Invalid token payload: {e}")))?;
        let claims: T = serde_json::from_slice(&payload_bytes)
            .map_err(|e| AppError::Auth(format!("Invalid token payload: {e}")))?;

        // ── Expiration check (Fix 2) ──
        let now = now_secs();
        if claims.expiration_secs() <= now {
            return Err(AppError::Auth("Token has expired".into()));
        }

        Ok(claims)
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Minimal base64url encode (no padding), no external crate.
fn base64_url_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() >= 2 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        }
        if chunk.len() >= 3 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        }
    }
    out
}

/// Minimal base64url decode (no padding), no external crate.
fn base64_url_decode(encoded: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(encoded.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits = 0u8;
    for ch in encoded.chars() {
        let val = match ch {
            'A'..='Z' => ch as u8 - b'A',
            'a'..='z' => ch as u8 - b'a' + 26,
            '0'..='9' => ch as u8 - b'0' + 52,
            '-' => 62,
            '_' => 63,
            _ => return Err(format!("Invalid base64url char: {ch}")),
        } as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_issues_and_verifies_access_token() {
        let svc = JwtService::new(b"test-secret-key-32bytes!!");
        let claims = AccessClaims {
            sub: "user-1".into(),
            email: "user@test.com".into(),
            name: "Test User".into(),
            roles: vec!["admin".into()],
            permissions: vec!["providers.manage".into()],
            teams: vec!["team-1".into()],
            tenant_scope: None,
            iat: 0,
            exp: 0,
            jti: String::new(),
            iss: String::new(),
        };

        let token = svc.issue_access(claims).unwrap();
        let decoded = svc.verify_access(&token).unwrap();

        assert_eq!(decoded.sub, "user-1");
        assert_eq!(decoded.email, "user@test.com");
        assert_eq!(decoded.roles, vec!["admin"]);
        assert_eq!(decoded.permissions, vec!["providers.manage"]);
        assert!(decoded.exp > decoded.iat);
    }

    #[test]
    fn it_issues_and_verifies_refresh_token() {
        let svc = JwtService::new(b"test-secret-key-32bytes!!");
        let (token, claims) = svc.issue_refresh("user-1").unwrap();

        let decoded = svc.verify_refresh(&token).unwrap();
        assert_eq!(decoded.sub, "user-1");
        assert_eq!(decoded.jti, claims.jti);
        assert!(decoded.exp > decoded.iat);
    }

    #[test]
    fn it_rejects_tampered_token() {
        let svc = JwtService::new(b"test-secret-key-32bytes!!");
        let claims = AccessClaims {
            sub: "user-1".into(),
            email: "u@t.com".into(),
            name: "U".into(),
            roles: vec![],
            permissions: vec![],
            teams: vec![],
            tenant_scope: None,
            iat: 0,
            exp: 0,
            jti: String::new(),
            iss: String::new(),
        };
        let token = svc.issue_access(claims).unwrap();

        // Tamper with the payload part
        let mut parts: Vec<&str> = token.splitn(3, '.').collect();
        parts[1] = "tampered_payload";
        let tampered = parts.join(".");

        let result = svc.verify_access(&tampered);
        assert!(result.is_err());
    }

    #[test]
    fn it_rejects_token_with_wrong_secret() {
        let svc1 = JwtService::new(b"secret-one-32-bytes-long!!");
        let svc2 = JwtService::new(b"secret-two-32-bytes-long!!");

        let claims = AccessClaims {
            sub: "user-1".into(),
            email: "u@t.com".into(),
            name: "U".into(),
            roles: vec![],
            permissions: vec![],
            teams: vec![],
            tenant_scope: None,
            iat: 0,
            exp: 0,
            jti: String::new(),
            iss: String::new(),
        };
        let token = svc1.issue_access(claims).unwrap();
        let result = svc2.verify_access(&token);
        assert!(result.is_err());
    }

    #[test]
    fn it_rejects_expired_token() {
        let svc = JwtService::new(b"test-secret-key-32bytes!!");
        // Manually construct an expired access token by encoding a claim
        // with exp in the past (issue_access always sets now+900, so we
        // bypass it for this test).
        let expired = AccessClaims {
            sub: "user-1".into(),
            email: "u@t.com".into(),
            name: "U".into(),
            roles: vec![],
            permissions: vec![],
            teams: vec![],
            tenant_scope: None,
            iat: 1000,
            exp: 1001, // way in the past
            jti: "test-jti".into(),
            iss: "llm_proxy".into(),
        };
        let token = svc.encode(&expired).unwrap();
        let result = svc.verify_access(&token);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expired"));
    }

    #[test]
    fn it_rejects_invalid_token_format() {
        let svc = JwtService::new(b"test-secret-key-32bytes!!");
        let result = svc.verify_access("not-a-jwt");
        assert!(result.is_err());
    }
}

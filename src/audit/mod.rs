//! Audit sidecar types — ADR-005 v2.
//!
//! All per-request observability fields are collected here, kept out of
//! `types` / `provider::chat` hot paths.  The server handler assembles three
//! audit sources at persist time and writes a single [`AuditDetail`] row.

use std::fmt;

// ── Cache kind ──────────────────────────────────────────────────────────

/// Identifies the upstream provider's cache-tracking convention.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ProviderCacheKind {
    #[default]
    None,
    OpenAiPromptCache,
    DeepSeekPromptCache,
    GeminiCachedContent,
    AnthropicCacheControl,
}

impl fmt::Display for ProviderCacheKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::OpenAiPromptCache => write!(f, "OpenAiPromptCache"),
            Self::DeepSeekPromptCache => write!(f, "DeepSeekPromptCache"),
            Self::GeminiCachedContent => write!(f, "GeminiCachedContent"),
            Self::AnthropicCacheControl => write!(f, "AnthropicCacheControl"),
        }
    }
}

// ── Cache report ────────────────────────────────────────────────────────

/// Cache-hit information extracted from upstream usage fields.
#[derive(Debug, Clone, Default)]
pub struct CacheReport {
    pub hit_tokens: Option<i64>,
    pub creation_tokens: Option<i64>,
    pub source: ProviderCacheKind,
}

impl CacheReport {
    pub fn none() -> Self {
        Self {
            hit_tokens: None,
            creation_tokens: None,
            source: ProviderCacheKind::None,
        }
    }
}

// ── Three audit sources ─────────────────────────────────────────────────

/// Provider-contributed audit fields.
#[derive(Debug, Clone, Default)]
pub struct AuditFromProvider {
    pub cache: CacheReport,
    pub reasoning_tokens: Option<i64>,
    pub audio_tokens: Option<i64>,
    pub upstream_model: Option<String>,
    pub system_fingerprint: Option<String>,
    pub finish_reason: Option<String>,
}

/// Router-contributed audit fields (populated by the handler from the
/// accumulated retry state and the stream inspector).
#[derive(Debug, Clone, Default)]
pub struct AuditFromRouter {
    pub retry_count: i32,
    pub ttft_ms: Option<i64>,
}

/// Auth-contributed audit fields.
#[derive(Debug, Clone)]
pub struct AuditFromAuth {
    pub tenant_id: String,
}

impl Default for AuditFromAuth {
    fn default() -> Self {
        Self {
            tenant_id: "default".to_string(),
        }
    }
}

/// Merged audit snapshot — the single struct handed to [`db::log_request`].
#[derive(Debug, Clone)]
pub struct AuditDetail {
    pub from_provider: AuditFromProvider,
    pub from_router: AuditFromRouter,
    pub from_auth: AuditFromAuth,
}

impl AuditDetail {
    pub fn none() -> Self {
        Self {
            from_provider: AuditFromProvider::default(),
            from_router: AuditFromRouter::default(),
            from_auth: AuditFromAuth::default(),
        }
    }
}

// ── Structured error code ───────────────────────────────────────────────

/// Structured error code extracted from [`AppError`](crate::error::AppError) at the
/// gateway level (never produced by the provider).
#[derive(Debug, Clone)]
pub enum ErrorCode {
    None,
    InvalidApiKey,
    RateLimitExceeded,
    ModelNotFound,
    ContentFilter,
    UpstreamTimeout,
    PoolExhausted,
    Other(String),
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::InvalidApiKey => write!(f, "InvalidApiKey"),
            Self::RateLimitExceeded => write!(f, "RateLimitExceeded"),
            Self::ModelNotFound => write!(f, "ModelNotFound"),
            Self::ContentFilter => write!(f, "ContentFilter"),
            Self::UpstreamTimeout => write!(f, "UpstreamTimeout"),
            Self::PoolExhausted => write!(f, "PoolExhausted"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

impl ErrorCode {
    /// Map an [`AppError`](crate::error::AppError) to its structured error code.
    pub fn from_app_error(err: &crate::error::AppError) -> Self {
        match err {
            crate::error::AppError::Auth(_) => Self::InvalidApiKey,
            crate::error::AppError::NotFound(_) => Self::ModelNotFound,
            crate::error::AppError::Upstream {
                status: Some(429), ..
            } => Self::RateLimitExceeded,
            crate::error::AppError::Upstream { status: None, .. } => Self::UpstreamTimeout,
            crate::error::AppError::Internal(msg) if msg.starts_with("all keys in pool") => {
                Self::PoolExhausted
            }
            _ => Self::Other(err.to_string()),
        }
    }
}

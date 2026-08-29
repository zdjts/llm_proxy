//! `llm_proxy` — a self-hosted OpenAI-compatible LLM gateway (v2.0).
//!
//! Exposes `POST /v1/chat/completions` and `GET /v1/models`, routing
//! requests across multiple upstream key pools with weighted-random
//! selection, background health probing, and per-request SQLite logging.
//!
//! v2.0 adds dynamic key management, per-tenant quotas, provider plugin
//! architecture, transform pipelines, multi-strategy routing, multi-level
//! caching, live request dashboard, and distributed state support.

pub mod aggregator;
pub mod alerts;
pub mod audit;
pub mod audit_trail;
pub mod auth;
pub mod auth_store;
pub mod bootstrap;
pub mod budget;
pub mod cache;
pub mod chat_service;
pub mod circuit_breaker;
pub mod concurrency;
pub mod config;
pub mod config_store;
pub mod dashboard;
pub mod db;
pub mod db_maintenance;
pub mod error;
pub mod fallback;
pub mod health;
pub mod health_check;
pub mod metrics;
pub mod model_catalog;
pub mod model_import;
pub mod pipeline;
pub mod provider;
pub mod quota;
pub mod ratelimit;
pub mod rbac;
pub mod redis_state;
pub mod response_validate;
pub mod router;
pub mod router_strategy;
pub mod runtime;
pub mod server;
pub mod sse_relay;
pub mod token_counter;
pub mod types;

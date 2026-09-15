//! `llm_proxy` — a self-hosted OpenAI-compatible LLM gateway (v2.0).
//!
//! Exposes `POST /v1/chat/completions` and `GET /v1/models`, routing
//! requests across multiple upstream key pools with weighted-random
//! selection, background health probing, and per-request SQLite logging.
//!
//! v2.0 adds dynamic key management, provider plugin
//! architecture, multi-strategy routing, caching, and a live request dashboard.

pub mod aggregator;
pub mod alerts;
pub mod audit;
pub mod audit_trail;
pub mod auth;
pub mod auth_store;
pub mod bootstrap;
pub mod cache;
pub mod chat_service;
pub mod circuit_breaker;
pub mod concurrency;
pub mod config;
pub mod config_store;
pub mod credential;
pub mod dashboard;
pub mod db;
pub mod db_maintenance;
pub mod error;
pub mod health;
pub mod health_check;
pub mod metrics;
pub mod model_catalog;
pub mod model_import;
pub mod provider;
pub mod ratelimit;
pub mod router;
pub mod runtime;
pub mod server;
pub mod types;

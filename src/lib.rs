//! `llm_proxy` — a self-hosted OpenAI-compatible LLM gateway.
//!
//! Exposes `POST /v1/chat/completions` and `GET /v1/models`, routing
//! requests across multiple upstream key pools with weighted-random
//! selection, background health probing, and per-request SQLite logging.

pub mod aggregator;
pub mod alerts;
pub mod audit;
pub mod auth;
pub mod cache;
pub mod config;
pub mod dashboard;
pub mod db;
pub mod error;
pub mod health;
pub mod provider;
pub mod ratelimit;
pub mod router;
pub mod server;
pub mod types;

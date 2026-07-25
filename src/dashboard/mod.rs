//! Dashboard submodule declarations (v2.0 — frontend separated).
//!
//! All askama SSR templates removed. Pure REST JSON + CSV endpoints.

pub mod admin_api;
pub mod alerts;
pub mod auth_api;
pub mod cost;
pub mod cost_drilldown;
pub mod csv;
pub mod export;
pub mod help;
pub mod keys;
pub mod live;
pub mod replay;
pub mod requests;
pub mod stub_api;
pub mod traffic;
pub mod ws;

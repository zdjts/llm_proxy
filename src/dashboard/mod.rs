//! Dashboard submodule declarations (v2.0 — frontend separated).
//!
//! All askama SSR templates removed. Pure REST JSON + CSV endpoints.

pub mod admin_api;
pub mod alerts;
pub mod cost;
pub mod cost_drilldown;
pub mod csv;
pub mod export;
pub mod keys;
pub mod live;
pub mod queries;
pub mod requests;
pub mod traffic;
pub mod usage;

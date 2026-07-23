//! Dashboard module — SSR via askama (ADR-007).
//!
//! All handlers return HTML rendered from compile-time-typed askama templates.
//! CSS and JS are inlined into `base.html`; no external static assets.

pub mod alerts;
pub mod cost;
pub mod cost_drilldown;
pub mod csv;
pub mod help;
pub mod keys;
pub mod layout;
pub mod requests;
pub mod traffic;

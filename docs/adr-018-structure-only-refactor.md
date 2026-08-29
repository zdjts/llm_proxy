# ADR-018: Structure-only refactor (keep product capabilities)

## Status

Accepted (planning). Implementation follows
`docs/implementation-plan-structure-refactor.md`.

## Context

`llm_proxy` accumulated many product surfaces (multi-provider routing, health
probes, SQLite logging, askama/admin dashboard, alerts, aggregator, prompt
cache, multi-tenant auth, ConfigStore hot-reload, frontend console, etc.).
A refactor was proposed to “delete some complexity.”

Two strategies were considered:

1. **Capability slim-down** — remove operational subsystems (dashboard, alerts,
   aggregator, cache, extra providers) to shrink the binary and mental model.
2. **Structure-only** — keep every product capability; reduce complexity by
   module boundaries, duplicate type/config collapse, and clearer startup
   wiring.

Existing related docs (`docs/decision-memo-personal-slim.md`,
`docs/implementation-plan-personal-slim.md`) describe the first strategy.
This ADR records the opposite choice for the current effort.

## Decision

**Keep all product capabilities. Refactor internal structure only.**

- Do not delete dashboard, alerts, aggregator, prompt cache, multi-provider
  support, health probing, multi-tenant alerts, ConfigStore, or admin APIs
  as part of this work.
- External contracts stay frozen:
  - `config.yaml` shape, including `client_keys` as `{key, tenant_id}` objects.
  - Public HTTP: `/v1/chat/completions`, `/v1/models`, and existing admin
    behavior (including IP-guard 404 for non-whitelisted admin).
  - Unknown JSON/YAML fields must still pass through (never
    `#[serde(deny_unknown_fields)]`).
- Allowed change: internal module moves, type merges, wiring cleanup.
- Forbidden: new LLM SDK crates, SPA/Tailwind for the askama dashboard,
  Redis as a new required dependency, plaintext keys in logs.

**Definition of done**

- Core module boundaries are explicit (config/bootstrap vs runtime store vs
  request pipeline vs dashboard/aggregation).
- Duplicate config/types that exist only because of copy-paste are merged.
- `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `cargo build --release` all pass.
- No intentional behavior delta; existing tests are the contract.

**First cut:** `Config` + application/runtime assembly (`src/config`,
startup wiring in `src/main.rs` / `src/runtime.rs` / `AppState` construction),
without changing YAML semantics. Request pipeline and dashboard come later.

## Consequences

- Scope is larger than a personal slim-down (code stays feature-complete) but
  safer for existing deployments (zero config/API migration).
- “Clear boundaries” is still somewhat subjective; the implementation plan
  lists target modules so PRs can be rejected if they only shuffle files.
- Large moves can still break tests, askama template paths, and admin IP
  guards; each slice must stay independently green.
- ADR-017 runtime authority (YAML bootstrap vs ConfigStore vs Router snapshot
  vs accounting pricing) remains in force; structure work must not collapse
  those authorities accidentally.

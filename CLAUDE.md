# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`llm_proxy` — a self-hosted, single-binary, OpenAI-API-compatible LLM gateway written in Rust (edition 2024). It proxies `/v1/chat/completions` and `/v1/models` across multiple upstream providers (OpenAI, Anthropic, Gemini, and any OpenAI-compatible backend) with weighted key pools, failover, rate limiting, quotas, caching, prompt/response transform pipelines, per-request SQLite audit logging, and a React admin dashboard.

The repo is currently mid-transition from a v0.x SSR-dashboard gateway to a `v2.0` rewrite (dynamic auth store, quota tracking, provider plugin registry, transform pipelines, multi-strategy routing, Redis-backed distributed state, React SPA dashboard). Code under `src/` reflects the current v2.0 state; treat `docs/` as historical design record, not always current — see "Docs are historical" below.

## Commands

```bash
# Build / run
cargo build --release
RUST_LOG=info cargo run --release          # reads ./config.yaml (or $LLM_PROXY_CONFIG)

# Required before every commit (all four must be clean)
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo build --release

# Single test
cargo test --test router_weighted                 # one integration test file
cargo test router_fallbacks_on_502                 # one test by name (any file)
cargo test --lib -- some_unit_test_name            # unit test in src/

# CLI management tool (requires `cli` feature)
cargo run --features cli --bin llm_proxy_cli -- status
cargo run --features cli --bin llm_proxy_cli -- client-keys list

# Frontend (React admin dashboard, served from frontend/dist in production)
cd frontend && npm install
npm run dev       # vite dev server
npm run build      # tsc -b && vite build -> frontend/dist, served by axum ServeDir
```

There are two binaries: `llm_proxy` (the gateway, `src/main.rs`) and `llm_proxy_cli` (`src/cli.rs`, gated behind the `cli` Cargo feature).

## Architecture

```
Client → axum middleware: request_id → auth (Bearer client_key) → rate_limit → quota → body_size_limit
         → handler resolves model → pool via config.model_to_pool
         → Router: weighted-random key selection from the pool, skipping BadKeyRegistry entries
         → Arc<dyn Provider>::chat(req, key)  (OpenAI / Anthropic / Gemini — router never matches concrete types)
         → on failure: failover retries next key (bad_status_codes) or circuit breaker trips
         → SQLite request_log INSERT (audit sidecar) + hourly aggregator UPSERT into audit_hourly
         → Admin dashboard (REST JSON + WebSocket, served to React SPA in frontend/dist)

Background tasks (spawned in main.rs, all shutdown-signal-aware):
  - health: re-probes and re-enables recovered keys in BadKeyRegistry
  - aggregator: hourly request_log → audit_hourly UPSERT
  - db_maintenance: periodic cleanup/VACUUM
  - alerts: broadcast::Receiver<AlertEvent> → SQLite persist + multi-channel dispatch (Webhook HMAC/Slack/Discord/Email)
```

### Provider abstraction (`src/provider/`)

`Provider` trait (`provider/mod.rs`) is the single extension point: `id()`, `base_url()`, `chat(req, key) -> ProviderResponse`, `probe()` for health checks, `extract_audit()` for the audit sidecar. `ProviderResponse` is either `Once(ChatCompletionResponse)` or `Stream { body: BoxStream<Result<Bytes, AppError>> }` — streaming bytes are relayed verbatim to the client, never re-serialized; `StreamInspector` (`provider/inspector.rs`) is a read-only sidecar that watches chunks for usage/audit accumulation.

Hard constraint carried over from the original design docs and still enforced: **the router and server layers must never `match` on concrete `Provider` types** — everything goes through `Arc<dyn Provider>`. New upstreams are added by implementing this trait and registering with `ProviderRegistry`/`ProviderFactory` (see the three factories at the bottom of `src/main.rs`).

### Config (`src/config/mod.rs`)

Strongly-typed `Config` loaded from YAML (`config.example.yaml` is the annotated template). Key sections: `server`, `auth.client_keys` (each a `{key, tenant_id}` object — tenant flows through auth, rate limiting, quotas, pricing, and dashboard slicing), `db`, `failover`, `pools` (weighted `KeyEntry` lists), `providers` (`kind: openai|anthropic|gemini`), `model_to_pool` (supports `default_params` per route), `admin` (IP allowlist), `pricing`, `rate_limit`, `concurrency`, `alerts.channels`.

### Routing (`src/router.rs`, `src/router_strategy.rs`)

Default strategy is weighted-random over a pool's `KeyEntry` list, excluding anything in `BadKeyRegistry`. `router_strategy.rs` adds pluggable `RoutingStrategy` variants (RoundRobin, LeastLatency, LeastConnections, CostOptimized, AdaptiveWeighted) that select over `PoolMetrics`/`KeyMetrics`.

### Transform pipeline (`src/pipeline.rs`)

`PreRequestTransform` / `PostResponseTransform` traits run before the provider call and after the response, respectively (e.g. `StripThinkingTransform`, `TruncateHistoryTransform`, `SystemPromptInjectTransform`, `JsonRepairTransform`). `Pipeline` is built once from `PipelineConfig` and held in `AppState`.

### Auth, quota, concurrency

- `auth_store.rs`: `AuthStore` (DashMap-backed) manages client keys at runtime, independent of the static `config.yaml` list — mutated via Admin API / CLI, not just SIGHUP reload.
- `quota.rs`: `QuotaTracker` enforces per-tenant daily-token/monthly-request caps via middleware.
- `concurrency.rs`: `ConcurrencyLimiter` caps per-tenant and total in-flight requests.
- `circuit_breaker.rs`: three-state (Closed/Open/HalfOpen) breaker layered on top of router failover.
- `auth/acl.rs`: per-tenant model allow/deny lists.

### Audit / logging (`src/audit/`, `src/db/`)

`AuditDetail` is assembled from three independently-filled parts — `AuditFromProvider` (cache/reasoning/audio/finish_reason/etc., filled by `Provider::extract_audit`), `AuditFromRouter` (retry_count, ttft_ms), `AuditFromAuth` (tenant_id) — merged by the handler right before the `request_log` INSERT. Never store raw provider JSON; every field the dashboard needs is a real column. `response_validate.rs` checks upstream responses conform to the OpenAI response schema before they're trusted downstream.

### Dashboard (`src/dashboard/`)

v2.0 removed the earlier askama SSR templates — dashboard is now pure REST JSON + WebSocket (`dashboard/ws.rs` for live key-health/request push, `dashboard/live.rs`, `dashboard/replay.rs` for request replay, `dashboard/export.rs` for JSON Lines/Parquet export, `dashboard/admin_api.rs` for client-key/quota/reload management), consumed by the React SPA in `frontend/`. Static assets are served from `frontend/dist/` via `tower_http::services::ServeDir`. IP-guard middleware still protects `/admin/*` by default.

### Distributed state (`src/redis_state.rs`, `src/cache/mod.rs`)

Cache supports a pluggable backend (in-memory `DashMap` LRU, or Redis) so BadKeyRegistry, rate limiter, prompt cache, and circuit breaker state can be shared across replicas — see `helm/llm_proxy/` and `k8s/` for cluster deployment manifests.

## Hard constraints (from `docs/BRIEF.md` / `docs/CONVENTIONS.md`, still enforced by review/clippy)

- No `unwrap()` / `expect()` / `panic!()` in request-handling paths (`server`, `router`, `provider`, `db`, `health`) — test code is exempt. If unavoidable, annotate with `// SAFETY: <reason>`.
- No plaintext upstream or client keys in logs, errors, responses, or the database — always `key_hash` (SHA-256, first 12 hex chars).
- All OpenAI-compatible DTOs must tolerate unknown fields (never `#[serde(deny_unknown_fields)]`) — unrecognized fields pass through unchanged.
- No LLM SDK crates (`async-openai`, `async-anthropic`, etc.) and no SSE-parsing crates — SSE framing is hand-rolled (see `provider/anthropic_stream.rs`, `provider/gemini_stream.rs`).
- `tracing` only for logging, no `println!`/`eprintln!` in `src/**` (a pre-tracing-init panic message in `main.rs` is the one exception) and no mixing with the `log` crate.
- `reqwest` with rustls only (`default-features = false`), never native-tls.
- Aggregator/health/alerts/db_maintenance background tasks must be shutdown-signal-aware and must not let a panic take down the process.

## Docs are historical, not authoritative

`docs/` contains the project's original design/process record: `CONVENTIONS.md` (style rules — still generally accurate and summarized above), `BRIEF.md` (a long, date-stamped task-dispatch log from v0.1 through v0.6, in Chinese), `GUIDE.md` (a full user/operator guide, but written for v0.8 — predates the v2.0 rewrite, so specifics like "6 dashboard screens" or SSR/askama no longer match `src/dashboard/`), `ROADMAP.md` (living document, has the most up-to-date feature inventory and the v2.0 architecture diagram), and numbered `adr-*.md` files (architecture decisions, one per milestone). When these disagree with the current code under `src/`, trust the code. `ROADMAP.md` §1's feature table is the most reliable single map from feature → file.

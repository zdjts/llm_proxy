# llm_proxy — AGENTS.md

## What it is

A self-hosted OpenAI-compatible LLM gateway written in Rust (edition 2024). Proxies `/v1/chat/completions` across multiple upstream key pools with weighted-random selection, health probing, failover, per-request SQLite logging, admin dashboard (askama SSR), rate limiting, prompt caching, multi-tenant alerts.

## Developer commands (exact — run in this order)

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Four must be green before any commit. All 132 tests pass.

## Config

- Runtime config: `config.yaml` (**not committed**, in `.gitignore`). Copy `config.example.yaml`.
- `LLM_PROXY_CONFIG` env var overrides the config path.
- `RUST_LOG` env var controls tracing level (default `info`).
- `client_keys` is an **array of objects** `[{key, tenant_id}]`, not plain strings. Old string-array format will fail at startup.

## Key conventions (violations will be rejected)

- **No LLM SDK crates** (`async-openai`, `async-anthropic`, etc.). No SSE parsing crates.
- No `unwrap()` / `expect()` / `panic!()` outside test code. `// SAFETY:` comment required for exceptions.
- No `println!` / `eprintln!` in `src/` (one boot-line in `main.rs` is allowed).
- No `std::sync::Mutex` held across `.await`. Use `tokio::sync::Mutex`.
- No plaintext keys in logs, errors, or responses — always `key_hash` (SHA-256 first 12 hex).
- All OpenAI-compatible DTOs must use `#[serde(deny_unknown_fields)]` — **never** set it. Unknown fields must pass through transparently.
- Router / server must never `match` on concrete Provider types — only through `Arc<dyn Provider>`.
- All test network calls must use `mockito`, never reach real external services.
- Admin routes behind IP guard (`config.admin.allowed_ips`); non-whitelisted requests get 404.

## Architecture

- **Entrypoint**: `src/main.rs` -> `src/lib.rs` (module declarations), `build.rs` embeds `RUNBOOK.md` as a compile-time constant.
- **Provider trait**: `src/provider/mod.rs`. Implementations: `openai.rs`, `anthropic.rs`, `gemini.rs`. `chat()` is one-shot (no retries). Failover/retry is in the router.
- **Router**: `src/router.rs` — weighted-random key selection via `WeightedIndex`. `BadKeyRegistry` (DashMap) tracks excluded keys. Health task re-enables them.
- **Server**: `src/server/mod.rs` — axum middleware chain: request_id -> auth -> rate_limit -> body_size_limit -> handler.
- **Dashboard**: `src/dashboard/` — askama SSR templates, inline CSS/JS/HTMX (no external assets). 6 screens (cost, requests, keys, traffic, alerts, cost drilldown).
- **DB**: SQLite via `sqlx`. Migrations in `migrations/` (0001-0005). `request_log` has 28 columns (14 base + 11 audit + local_cache_hit + others). `audit_hourly` aggregated table for dashboard.
- **Aggregator**: `src/aggregator/` — hourly background task, UPSERTs into `audit_hourly`, restart-idempotent.

## Commit style

Trunk-based: commit directly to `main`, no PRs, no force-push. Conventional Commits with scope:
`feat(provider):`, `fix(server):`, `test(auth):`, `chore(deps):`, `refactor(router):`, `docs(adr):`.

Commit body must contain a `verify:` block with all four command outputs.

## Test structure

- 66 unit tests in `src/` (`#[cfg(test)] mod tests`)
- 66 integration tests in `tests/*.rs`
- Tests use `mockito` for HTTP mocking, `tempfile` for SQLite paths, `tower::ServiceExt` for axum router testing.
- Failing tests: `tests/alerts.rs` can be slow (~10s, broadcast channel timeouts).

## Generated / build artifacts

- `target/`, `.env`, `config.yaml`, `data/` are gitignored.
- RUNBOOK.md is compiled into the binary via `build.rs` (served at `/admin/help`).
- askama templates are compiled into the binary (no runtime template loading).

## Not in scope for this repo

- No SPA / Tailwind / external frontend libraries for the dashboard.
- No CSV crate (CSV is hand-rolled in `src/dashboard/csv.rs`).
- Email alert channel is a stub (no SMTP).
- No distributed locking / Redis dependency.

# llm_proxy — AGENTS.md

## What it is

A self-hosted OpenAI-compatible LLM gateway in Rust (edition 2024). Proxies `/v1/chat/completions` across multiple upstream key pools with weighted-random selection, health probing, failover, per-request SQLite logging, admin dashboard (askama SSR), rate limiting, prompt caching, multi-tenant alerts.

## Developer commands (exact — run in this order)

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

All four must pass before commit. All 132 tests pass (66 unit in `src/`, 66 integration in `tests/`).

## Config

- Runtime: `config.yaml` (gitignored, copy `config.example.yaml`). Override via `LLM_PROXY_CONFIG` env var.
- Logging: `RUST_LOG` env var (default `info`).
- `client_keys` is an **array of objects** `[{key, tenant_id}]`, not plain strings.

## Key conventions (violations are rejected)

- **No LLM SDK crates** (`async-openai`, `async-anthropic`, etc.). No SSE parsing crates.
- No `unwrap()` / `expect()` / `panic!()` outside test code. `// SAFETY:` comment required for exceptions.
- No `println!` / `eprintln!` in `src/` (one boot-time line in `main.rs` is allowed).
- No `std::sync::Mutex` held across `.await`. Use `tokio::sync::Mutex`.
- No plaintext keys in logs, errors, or responses — always `key_hash` (SHA-256 first 12 hex).
- **Never** set `#[serde(deny_unknown_fields)]` — unknown fields must pass through transparently.
- Router/server must never `match` on concrete Provider types — only `Arc<dyn Provider>`.
- All test HTTP calls must use `mockito`, never real external services.
- Admin routes behind IP guard; non-whitelisted requests get 404.

## Architecture

- **Entrypoint**: `src/main.rs` → `src/lib.rs` (module declarations). `build.rs` embeds `RUNBOOK.md` as a compile-time constant.
- **Provider trait**: `src/provider/mod.rs`. Implementations: `openai.rs`, `anthropic.rs`, `gemini.rs`. `chat()` is one-shot (no retries). Failover/retry in the router.
- **Router**: `src/router.rs` — weighted-random key selection via `rand::WeightedIndex`. `BadKeyRegistry` (DashMap) tracks excluded keys; health task re-enables them.
- **Server**: `src/server/mod.rs` — axum middleware: request_id → auth → rate_limit → body_size_limit → handler. Handles `POST /v1/chat/completions` and `GET /v1/models`.
- **Dashboard**: `src/dashboard/` — askama SSR templates with inline CSS/JS/HTMX (zero external assets). 7 screens: cost, requests, keys, traffic, alerts, cost/drilldown, help. `/admin/help` serves the compiled RUNBOOK.
- **DB**: SQLite via `sqlx` with WAL mode. 26-column `request_log` table (migrations 0001–0005). `audit_hourly` for dashboard aggregation.
- **Aggregator**: `src/aggregator/` — hourly UPSERT into `audit_hourly`, restart-idempotent.
- **VCS**: Jujutsu (`jj`) alongside git; `.jj/` directory present.

## Test structure

- Tests use `mockito` (HTTP mocking), `tempfile` (SQLite paths), `tower::ServiceExt` (axum router testing).
- `tests/alerts.rs` can be slow (~10s, broadcast channel timeouts).

## Constraints

- No SPA / Tailwind / external frontend libs for dashboard.
- CSV is hand-rolled in `src/dashboard/csv.rs` (no CSV crate).
- Email alert channel is a stub (no SMTP).
- No distributed locking / Redis dependency.

## Commit style

Trunk-based (commit directly to `main`). Conventional Commits with scope: `feat(provider):`, `fix(server):`, `test(auth):`, etc. Body must contain a `verify:` block with all four command outputs.

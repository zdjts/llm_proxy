# llm_proxy — AGENTS.md

## What it is

A self-hosted OpenAI-compatible LLM gateway in Rust (edition 2024). Proxies `/v1/chat/completions` across multiple upstream key pools with weighted-random selection, health probing, failover, per-request SQLite logging, admin dashboard (**React SPA in `frontend/`, served as static `frontend/dist`**), rate limiting, prompt caching, multi-tenant alerts.

## Developer commands (exact — run in this order)

```bash
cargo fmt -- --check
cargo clippy --all-targets --features cli -- -D warnings
cargo test
cargo build --release --features cli
```

All four must pass before commit. Rust suite: 302 passing (unit in `src/`, integration in `tests/`).

Frontend (`frontend/`, vitest + @testing-library/react):

```bash
cd frontend && npm run build   # tsc -b && vite build → frontend/dist
cd frontend && npx vitest run  # 11 files / 28 tests
```

**The gateway serves `frontend/dist` statically** (`ServeDir::new("frontend/dist")` fallback in `src/server/mod.rs`), so `npm run build` is mandatory after any UI change — editing `frontend/src` alone does not change what users see.

### Full build convention (AI agents: always follow)

When building or verifying this project, **always do a complete build with the `cli`
feature enabled** — never a partial build, and never two `cargo run` invocations:

```bash
cargo build --release --features cli
```

Rules:

- **Never** run `cargo run --bin llm_proxy` / `cargo run --bin llm_proxy_cli` as the
  build step. Running the second binary without `--features cli` changes the feature
  set and forces a full recompile (`lto = true`, `codegen-units = 1` make this ~20s).
  One `cargo build --release --features cli` produces both binaries; then invoke them
  directly via the repo-root symlinks:
  - `./llm_proxy` (gateway)
  - `./llm_proxy_cli <subcommand>` (e.g. `./llm_proxy_cli status`, `./llm_proxy_cli keys`)
- The pre-commit sequence is therefore:

  ```bash
  cargo fmt -- --check
  cargo clippy --all-targets --features cli -- -D warnings
  cargo test
  cargo build --release --features cli
  ```

  `clippy` also needs `--features cli`, otherwise the `cli.rs` code
  (`#[cfg(feature = "cli")]`) is silently skipped.
- Do not "optimize" by dropping `--features cli` or `--release` to save time. A green
  clippy/test run that excluded the CLI binary is not a valid verification.
- To actually run the gateway after a build, use `./llm_proxy`, not `cargo run`.

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

- **Entrypoint**: `src/main.rs` → `src/lib.rs` (module declarations). No `build.rs` (removed with the askama SSR dashboard).
- **Provider trait**: `src/provider/mod.rs`. Implementations: `openai.rs`, `anthropic.rs`, `gemini.rs`. `chat()` is one-shot (no retries). Failover/retry in the router.
- **Router**: `src/router.rs` — weighted-random key selection via `rand::WeightedIndex`. `BadKeyRegistry` (DashMap) tracks excluded keys; health task re-enables them.
- **Server**: `src/server/mod.rs` — axum middleware: request_id → auth → rate_limit → body_size_limit → handler. Handles `POST /v1/chat/completions` and `GET /v1/models`.
- **Dashboard backend**: `src/dashboard/` — pure REST JSON + CSV endpoints (all askama SSR templates removed). Backs the React SPA in `frontend/` (7 screens: cost, requests, keys, traffic, alerts, cost/drilldown, help).
- **DB**: SQLite via `sqlx` with WAL mode. `request_log` table (19 migrations, 0001–0019). `audit_hourly` for dashboard aggregation.
- **Aggregator**: `src/aggregator/` — hourly UPSERT into `audit_hourly`, restart-idempotent.
- **VCS**: Jujutsu (`jj`) alongside git; `.jj/` directory present.
- **Container**: `Dockerfile` is 3-stage — `node:22-slim` builds `frontend/dist`, then `rust:1.96-slim-bookworm` builds the gateway, then `debian:bookworm-slim` runtime. `.dockerignore` excludes `target/`, `data/`, `frontend/node_modules/`, `frontend/dist/`. The Rust stage needs `pkg-config`, `libssl-dev`, **and `libsqlite3-dev`** (sqlx links system SQLite). `deb.debian.org` is unreachable in some networks; the Dockerfile writes a mirror into `debian.sources`, overridable via `--build-arg APT_MIRROR=...`.

## Test structure

- Tests use `mockito` (HTTP mocking), `tempfile` (SQLite paths), `tower::ServiceExt` (axum router testing).
- `tests/alerts.rs` can be slow (~10s, broadcast channel timeouts).
- Frontend tests: `vitest` + `@testing-library/react` + `jsdom`, colocated as `frontend/src/**/*.test.tsx`. API layer is mocked via `vi.mock('@/lib/api')`; locale is chosen by `localStorage.setItem('dashboard-locale', ...)`.

## Constraints

- CSV is hand-rolled in `src/dashboard/csv.rs` (no CSV crate).
- Email alert channel is a stub (no SMTP).
- No distributed locking / Redis dependency.

## Commit style

Trunk-based (commit directly to `main`). Conventional Commits with scope: `feat(provider):`, `fix(server):`, `test(auth):`, etc. Body must contain a `verify:` block with all four command outputs.

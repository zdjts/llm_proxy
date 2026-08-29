# Implementation plan: structure-only refactor

Companion to [ADR-018](adr-018-structure-only-refactor.md).

**Goal:** Keep every product capability of `llm_proxy`; reduce internal
complexity by splitting modules, merging duplicate types/config, and
straightening startup wiring.

**Non-goals:** Deleting dashboard, alerts, aggregator, cache, providers,
health probes, or multi-tenant features. Changing `config.yaml` or public
HTTP. Adding LLM SDKs, SPA dashboards, or Redis as a required dependency.

**Audience:** Maintainers / coding agents implementing follow-up PRs.

## Constraints (must preserve)

From `AGENTS.md` and ADR-018:

- `client_keys`: array of `{key, tenant_id}` objects, not strings.
- Never `#[serde(deny_unknown_fields)]`.
- No `unwrap`/`expect`/`panic!` outside tests.
- No `println!` in `src/` except the allowed boot line in `main.rs`.
- No `std::sync::Mutex` held across `.await`.
- Keys in logs/errors/responses: `key_hash` only.
- Router/server match only `Arc<dyn Provider>`, never concrete provider types.
- Tests: `mockito` only, no live upstreams. Admin routes: IP guard → 404.
- ADR-017 authorities stay: YAML bootstrap vs `ConfigStore` vs live `Router`
  vs accounting pricing vs process-local `AuthStore`.

## Verification (every slice)

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Stop the slice if any fail or if behavior tests need “updates” that encode a
product change.

## Current hotspots (why Config/AppState first)

Observed shape (not a full coupling map):

| Area | Symptom |
|---|---|
| `src/lib.rs` | ~40 top-level modules; easy to re-export cycles. |
| `src/config/mod.rs` (~820 lines) | Root `Config` owns many subsystem structs; some types live in feature modules (`auth`, `fallback`) and some in `config`. |
| `src/main.rs` (~685 lines) | Boot wiring: load YAML, DB, stores, router, background tasks. |
| `src/runtime.rs` | Small policy carrier; easy to confuse with `ConfigStore` snapshots. |
| `src/config_store.rs` | Runtime SQLite source for pools/providers/routing (ADR-017). |
| Request path | `server` + `chat_service` + `router` + `pipeline` + `fallback` + cache. |
| Ops path | `dashboard` + `aggregator` + `alerts` + `audit`. |

First cut attacks **config model + boot wiring**, because it is the fan-in
point and can stay behavior-neutral if YAML serde and ADR-017 stay untouched.

## Slice 0 — Inventory (read-only, same PR or precede slice 1)

- List every type deserialized from `config.yaml` and where it is defined.
- List `AppState` / shared handles constructed in `main.rs` and who owns them.
- Note duplicate names (`health` vs `health_check`, `auth` vs `auth_store`,
  `router` vs `router_strategy`).
- Do **not** move files in this slice if it would mix with behavior edits.

**Exit:** a short table in the PR description: type → module → runtime owner.

## Slice 1 — Config + AppState assembly (first implementation cut)

**In scope**

- Keep `Config` serde stable. Move subsystem config structs to a consistent
  home (`config` submodule **or** feature module with `config` re-export —
  pick one rule and apply it).
- Extract boot assembly from `main.rs` into a dedicated function/module
  (e.g. `runtime::bootstrap` or `app::build`) that returns the shared state
  the axum router needs. `main.rs` should parse flags/env, call bootstrap,
  serve, and spawn tasks — not construct every store inline.
- Make `RuntimePolicy` / failover / rate-limit / cache-max / alert thresholds
  clearly **startup-static** (ADR-017); do not invent hot-reload.
- Document in code comments which handles are YAML-only vs `ConfigStore`.

**Out of scope**

- Changing YAML keys, defaults, or validation error text unless tests already
  pin them and you only relocate the same checks.
- Dashboard templates, provider `chat()` implementations, SQL schema.

**Exit**

- Four commands green.
- `config.example.yaml` unchanged in meaning.
- `main.rs` thinner; bootstrap unit-testable without binding a port if
  practical (do not add heavy new test harnesses).

## Slice 2 — Request pipeline (after slice 1 is green)

Clarify stages without changing outcomes:

`request_id → auth → rate_limit → body_size → handler → router/provider → log`

- Keep failover/retry in the router, not in `Provider::chat`.
- Do not match concrete Provider types in server/router.
- Leave SSE/stream behavior as-is (`sse_relay`).

## Slice 3 — Dashboard / aggregator / alerts (last)

- Deduplicate SQL/query helpers used by cost/requests/keys/traffic/alerts.
- Do not change HTMX/askama UX or CSV hand-roll.
- Do not merge accounting pricing with `model_metadata` display pricing
  (ADR-017).

## Stop / reject criteria

- PR changes `config.yaml` field names or `client_keys` shape.
- PR removes a product module “to simplify.”
- Tests are rewritten to accept new status codes, auth failures, or admin 404
  rules.
- Clippy warnings allowed with `#[allow]` instead of fixing.
- Template path or `RUNBOOK.md` embed (`build.rs`) broken.

## Suggested commit style

Trunk-based, conventional commits, e.g. `refactor(config): extract bootstrap`.
Body must include a `verify:` block with the four command outputs.

## Order of work

1. Slice 0 inventory  
2. Slice 1 Config + AppState (**do this first**)  
3. Slice 2 pipeline  
4. Slice 3 ops/dashboard  

Do not start slice 2/3 in the same PR as slice 1.

# llm_proxy

A self-hosted OpenAI-compatible LLM gateway written in Rust.

Proxies `POST /v1/chat/completions` and `GET /v1/models` across multiple upstream key pools with weighted-random selection, health probing, failover, per-request SQLite logging, an admin dashboard (askama SSR), rate limiting, prompt caching, and multi-tenant alerts.

## Features

- **Multi-provider**: OpenAI, Anthropic, Gemini — all behind a single `Arc<dyn Provider>` trait
- **Weighted-random key pools**: distribute traffic across keys with arbitrary weights; bad keys are auto-excluded
- **Health probing**: background task re-enables recovered keys after configurable intervals
- **Failover**: bad-status codes (401/402/403/429) trigger key exclusion and retry; 5xx retry without exclusion
- **Streaming end-to-end**: SSE passthrough with usage tracking via `StreamInspector`
- **Per-request logging**: 28-column SQLite `request_log` with audit sidecar (cache, reasoning, audio, TTFT, etc.)
- **Hourly aggregation**: background aggregator UPSERTs into `audit_hourly` for dashboard queries
- **Admin dashboard**: 6 SSR screens (cost, requests, keys, traffic, alerts, cost drilldown) with IP guard, tenant slicing, and CSV export — no external JS/CSS, HTMX inlined
- **Rate limiting**: in-memory token bucket per tenant
- **Prompt caching**: local `DashMap` LRU cache with TTL; cache hits bypass upstream entirely
- **Multi-tenant auth**: `client_keys` as `{key, tenant_id}` objects; tenant context flows through audit and rate limiting
- **Alert channels**: Webhook (HMAC-signed), Slack, Discord, Email (stub) — configurable error burst and latency thresholds
- **No LLM SDK crates**: zero `async-openai` / `async-anthropic` / SSE parsing crates

## Quick start

```bash
# 1. Copy the example config and edit it
cp config.example.yaml config.yaml

# 2. Start the gateway
RUST_LOG=info cargo run --release
```

The server listens on `http://0.0.0.0:8080` by default.

## Configuration

See `config.example.yaml` for the full schema. Key sections:

| Section | Description |
|---------|-------------|
| `server` | Bind address, port, max body size |
| `auth` | Client API keys with tenant IDs |
| `db` | SQLite path |
| `failover` | Bad-status codes, probe interval/retries |
| `pools` | Key pools with weighted entries |
| `providers` | Upstream definitions (OpenAI / Anthropic / Gemini) |
| `model_to_pool` | Model → pool routing map |
| `admin` | Dashboard IP whitelist |
| `pricing` | Per-model USD pricing (prompt / completion) |
| `rate_limit` | Per-tenant RPM |
| `alerts` | Alert channels and thresholds |

Environment variables:
- `LLM_PROXY_CONFIG` — override config path (default: `config.yaml`)
- `RUST_LOG` — tracing level (default: `info`)

## API

### `POST /v1/chat/completions`

OpenAI-compatible chat completions. Supports streaming (`stream: true`) and non-streaming. Accepts unknown fields transparently.

### `GET /v1/models`

Lists all configured models.

### Admin dashboard

Mounted at `/admin/` when `admin.enabled: true`. IP-guarded (default: `127.0.0.1`, `::1`).

| Route | Screen |
|-------|--------|
| `/admin` | Cost overview with per-model table |
| `/admin/requests` | Request log with filters |
| `/admin/keys` | Key health with 7d sparkline |
| `/admin/traffic` | Traffic trend SVG chart |
| `/admin/alerts` | Recent alert events |
| `/admin/cost/drilldown` | Per-model cost drilldown |

Supports `?tenant=` slicing and `?format=csv` where applicable.

## Architecture

```
Client → axum middleware: [request_id → auth → rate_limit → body_size_limit]
         → Router (weighted key selection)
         → Provider (OpenAI / Anthropic / Gemini)
         → Upstream API
         → SQLite log + Aggregator (hourly)
         → Admin dashboard (askama SSR)

Background tasks:
  - Health probe: re-enables recovered keys
  - Aggregator: hourly audit_hourly UPSERT
  - Alert dispatcher: multi-channel delivery
```

Key design constraints:
- Router/server never match on concrete `Provider` types
- All OpenAI DTOs use `#[serde(deny_unknown_fields)]` — never set; unknown fields pass through
- No `unwrap()` / `expect()` / `panic!()` in non-test code
- No plaintext keys in logs, errors, or responses — always `key_hash` (SHA-256 first 12 hex)

## Development

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

All 132 tests pass. Test infrastructure uses `mockito` for HTTP mocking, `tempfile` for SQLite, and `tower::ServiceExt` for axum router testing.

## Built with

- **axum** 0.8 — HTTP framework
- **sqlx** 0.8 — SQLite with compile-time queries
- **askama** 0.12 — SSR templates compiled into binary
- **reqwest** 0.12 — upstream HTTP client
- **tower-http** — request body limit layer
- **DashMap** — concurrent key-value for bad-key registry and rate limiting

## License

MIT

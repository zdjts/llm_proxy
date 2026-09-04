# ADR-017: Runtime and pricing authority boundaries

## Scope

This document records the intentionally separate runtime sources used by the
HTTP gateway. It does not change the external API or the pricing schema.

## Authorities and update behavior

- Startup YAML provides bootstrap/static values only. Managed `pools`, `providers`, and model routing are loaded from SQLite by `ConfigStore`; YAML is not synchronized or written back automatically.
- After startup synchronization, `ConfigStore`/SQLite is the runtime source for
  pools, providers, and model routing. A successful poll and router rebuild
  replaces the complete snapshot; a failed refresh or rebuild keeps the prior
  snapshot active.
- The live `Router` snapshot is the only authority for callable models. The
  model catalog projects `/v1/models` and `/v1/model-metadata` from that
  snapshot, so metadata configuration cannot create a route.
- `model_metadata` is initialized from YAML into the ConfigStore snapshot. The
  model catalog reads this single snapshot and projects metadata only onto live
  Router models. Metadata pricing is display-only USD per million tokens.
- Accounting, budget checks, request cost logging, and cost dashboards use
  the explicit `ConfigStore` pricing carrier initialized from validated startup
  configuration. The `AccountingPricing` boundary makes this source explicit.
  Metadata pricing is not converted, merged, or allowed to override accounting
  pricing; the two structures have different compatibility and unit semantics.
- The DB `model_registry` stores display/capability metadata for admin and
  catalog enrichment. Explicit admin config export/import may carry these rows
  so deployments round-trip completely, but the registry is still not a source
  of callable models, metadata routes, or accounting prices. A model becomes
  callable only after it has a live `routing_config` entry (and therefore a
  Router snapshot entry). Creating or updating a registry row with an enabled
  `provider_config_id` may auto-create that routing row from the provider's
  pool.

- Client authentication is a bootstrap + ephemeral hybrid. YAML client keys
  seed the shared `AuthStore` at startup; admin CRUD changes only its process
  local state. `client_key_store` contains hashes and metadata but no recoverable
  plaintext key, so runtime-created/rotated/deleted state is not restored after
  restart. When the store exists, authentication never falls back to bootstrap
  entries. Client-key responses and audit records expose hashes/metadata only.

- Rate-limit, concurrency, fallback, and pipeline settings are startup-static
  service inputs from the validated YAML bootstrap; no DB carrier or hot reload
  exists for them in this phase. Prompt-cache policy is also startup-static;
  cache contents and hit statistics are transient runtime state. Failover and
  alert thresholds are exposed through the explicit `RuntimePolicy` bootstrap
  carrier, while bad-key/probe and alert-trigger state remain transient and are
  never exported as configuration.

## Client-key API contract

`POST /admin/api/client-keys` and rotation accept plaintext only in the request
and return a hash/metadata projection. List, update, delete responses, audit
records, exports, logs, and errors must not contain plaintext key material.
Create/rotate/delete/disable state is process-local and is lost on restart;
operators must re-provision runtime keys through the explicit management API.


Admin DB changes to pools, providers, and model routing can hot-update
`ConfigStore` and the live router after the poller/rebuilder succeeds. They do
not automatically update YAML metadata, pricing, authentication, rate limits,
alerts, or other startup-fixed components.

Configuration changes outside the explicit admin import/refresh flows do not
update YAML metadata, pricing, authentication, rate limits, alerts, or other
startup-fixed components. The legacy signal/reload path is not a runtime
configuration source. Runtime catalog reads use only the ConfigStore snapshot;
future unification of metadata and accounting prices requires an explicit
migration, unit conversion decision, and billing regression tests.

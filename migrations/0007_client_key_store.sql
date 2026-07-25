-- Migration 0007: client_key_store table for dynamic key management (v2.0 Module A1)
CREATE TABLE IF NOT EXISTS client_key_store (
    key_hash TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    created_at INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    label TEXT NOT NULL DEFAULT ''
);

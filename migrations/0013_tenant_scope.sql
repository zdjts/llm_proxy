-- 0013: Add tenant_scope to user_account (missed in migration 0009)
-- Part of v3.0 full-stack upgrade per docs/BRIEF-v3.0-fullstack-upgrade.md

ALTER TABLE user_account ADD COLUMN tenant_scope TEXT;

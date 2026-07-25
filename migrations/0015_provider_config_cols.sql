-- 0015: Add missing columns to provider_config (T172 supplement)
-- provider_config was created in migration 0008 without weight and
-- bad_status_codes_override. These are needed for ConfigStore (Track H).
--
-- SQLite only supports ADD COLUMN in ALTER TABLE, which is sufficient here.

ALTER TABLE provider_config ADD COLUMN weight INTEGER NOT NULL DEFAULT 1;
ALTER TABLE provider_config ADD COLUMN bad_status_codes_override TEXT;  -- JSON array or NULL

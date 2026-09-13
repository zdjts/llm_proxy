-- 0018: OAuth credential fields on upstream key_entry.
-- key_hash stays a stable identity (hash of refresh token for oauth, api key otherwise)
-- and is not rewritten when the access token rotates.
ALTER TABLE key_entry ADD COLUMN cred_type TEXT NOT NULL DEFAULT 'api_key';
ALTER TABLE key_entry ADD COLUMN refresh_token TEXT;
ALTER TABLE key_entry ADD COLUMN expires_at INTEGER;
ALTER TABLE key_entry ADD COLUMN issuer TEXT;

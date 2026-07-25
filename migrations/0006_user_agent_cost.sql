-- ADR-016: user_agent and cost_usd columns, remove verbose header debug logging.
ALTER TABLE request_log ADD COLUMN user_agent TEXT DEFAULT NULL;
ALTER TABLE request_log ADD COLUMN cost_usd REAL DEFAULT NULL;
CREATE INDEX IF NOT EXISTS idx_request_log_user_agent ON request_log(user_agent);
CREATE INDEX IF NOT EXISTS idx_request_log_cost_usd ON request_log(cost_usd);

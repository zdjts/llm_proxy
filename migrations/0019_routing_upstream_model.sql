-- Optional client-facing alias: logical_model is what callers send;
-- upstream_model is rewritten onto the request before the provider call.
ALTER TABLE routing_config ADD COLUMN upstream_model TEXT;

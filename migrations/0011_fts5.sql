-- 0011: FTS5 full-text search on request_log messages (T133)
-- Part of v3.0 full-stack upgrade per docs/BRIEF-v3.0-fullstack-upgrade.md
--
-- FTS5 virtual table for searching messages content in request_log.
-- The content table approach means we store an external content table
-- and FTS5 indexes the text but does not duplicate the storage.
-- Populated lazily via a background task to avoid blocking writes.

CREATE VIRTUAL TABLE IF NOT EXISTS request_log_fts USING fts5(
    request_id UNINDEXED,      -- foreign key to request_log.id
    messages_text,              -- extracted text from all messages (roles + content)
    content='',                 -- content-less: all data in the FTS index itself
    tokenize='unicode61'
);

-- Index to track which rows have been FTS5-indexed to allow incremental updates
CREATE TABLE IF NOT EXISTS request_log_fts_watermark (
    id          INTEGER PRIMARY KEY,
    last_rowid  INTEGER NOT NULL DEFAULT 0       -- last request_log rowid processed
);
INSERT OR IGNORE INTO request_log_fts_watermark (id, last_rowid) VALUES (1, 0);

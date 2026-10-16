-- Migration v1: Initial schema for FindX

-- 1. FTS5 Full-Text Search Virtual Table
CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
    path UNINDEXED,
    filename,
    content,
    mime_type,
    tags,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- 2. Frecency Engagement Table
CREATE TABLE IF NOT EXISTS frecency_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id TEXT NOT NULL,
    item_type TEXT NOT NULL, -- 'app' or 'file'
    event_type TEXT NOT NULL, -- 'launch', 'open', 'reveal'
    timestamp INTEGER NOT NULL, -- epoch seconds
    query TEXT
);
CREATE INDEX IF NOT EXISTS idx_frecency_item ON frecency_events(item_id, timestamp);

-- 3. Tag Cache Table
CREATE TABLE IF NOT EXISTS tag_cache (
    path TEXT NOT NULL,
    tag_name TEXT NOT NULL,
    tag_color TEXT,
    mtime INTEGER NOT NULL,
    PRIMARY KEY (path, tag_name)
);
CREATE INDEX IF NOT EXISTS idx_tag_name ON tag_cache(tag_name);

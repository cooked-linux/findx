-- Migration v2: File records table for tracking indexed files metadata

CREATE TABLE IF NOT EXISTS file_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id INTEGER NOT NULL,
    inode INTEGER NOT NULL,
    path TEXT NOT NULL UNIQUE,
    filename TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    modified_time INTEGER NOT NULL,
    indexed_time INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_file_records_path ON file_records(path);
CREATE INDEX IF NOT EXISTS idx_file_records_dev_ino ON file_records(device_id, inode);

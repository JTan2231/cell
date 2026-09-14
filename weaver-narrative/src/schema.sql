CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    request TEXT NOT NULL,
    markdown TEXT,
    pending_reply TEXT,
    finished_at INTEGER,
    error TEXT
);
PRAGMA user_version=1;

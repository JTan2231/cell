CREATE TABLE crm_meta_v2 (
    marker TEXT PRIMARY KEY CHECK (marker = 'crm'),
    schema_version INTEGER NOT NULL CHECK (schema_version = 2),
    worker_token TEXT,
    worker_pid INTEGER,
    worker_acquired_at TEXT,
    CHECK ((worker_token IS NULL) = (worker_pid IS NULL)),
    CHECK ((worker_token IS NULL) = (worker_acquired_at IS NULL)),
    CHECK (worker_token IS NULL OR length(worker_token) > 0),
    CHECK (worker_pid IS NULL OR worker_pid > 0)
) STRICT;
INSERT INTO crm_meta_v2
    (marker, schema_version, worker_token, worker_pid, worker_acquired_at)
SELECT marker, 2, worker_token, worker_pid, worker_acquired_at FROM crm_meta;
DROP TABLE crm_meta;
ALTER TABLE crm_meta_v2 RENAME TO crm_meta;

CREATE TABLE profile_entries (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    body_md TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

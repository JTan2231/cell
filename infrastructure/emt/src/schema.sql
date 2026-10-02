PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
CREATE TABLE incidents (
    id TEXT PRIMARY KEY,
    binding_key TEXT NOT NULL,
    feed_cursor INTEGER NOT NULL,
    reply_to TEXT NOT NULL UNIQUE,
    clockwork_json TEXT NOT NULL,
    basic_email_json TEXT NOT NULL
) STRICT;
CREATE TABLE exchanges (
    id TEXT PRIMARY KEY,
    incident_id TEXT NOT NULL REFERENCES incidents(id),
    kind TEXT NOT NULL CHECK(kind IN ('diagnosis','reply')),
    incoming_id TEXT UNIQUE,
    incoming_json TEXT,
    nucleus_job_id TEXT NOT NULL UNIQUE,
    request_json TEXT,
    request_digest TEXT,
    job_admitted INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','running','finished','failed')),
    created_at INTEGER NOT NULL,
    deadline_at INTEGER NOT NULL,
    mail_json TEXT,
    send_state TEXT CHECK(send_state IN ('pending','accepted','uncertain')),
    first_send_at INTEGER,
    last_send_at INTEGER,
    send_attempts INTEGER NOT NULL DEFAULT 0,
    email_id TEXT,
    error TEXT
) STRICT;
CREATE UNIQUE INDEX one_diagnosis ON exchanges(incident_id) WHERE kind='diagnosis';
PRAGMA user_version = 1;
COMMIT;

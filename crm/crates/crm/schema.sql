PRAGMA foreign_keys = ON;

CREATE TABLE crm_meta (
    marker TEXT PRIMARY KEY CHECK (marker = 'crm'),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    worker_token TEXT,
    worker_pid INTEGER,
    worker_acquired_at TEXT,
    CHECK ((worker_token IS NULL) = (worker_pid IS NULL)),
    CHECK ((worker_token IS NULL) = (worker_acquired_at IS NULL)),
    CHECK (worker_token IS NULL OR length(worker_token) > 0),
    CHECK (worker_pid IS NULL OR worker_pid > 0)
) STRICT;
INSERT INTO crm_meta(marker, schema_version, worker_token, worker_pid, worker_acquired_at)
VALUES ('crm', 1, NULL, NULL, NULL);

CREATE TABLE cases (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    head_revision INTEGER NOT NULL CHECK (head_revision >= 1),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE deliveries (
    id TEXT PRIMARY KEY,
    case_id TEXT NOT NULL REFERENCES cases(id),
    label TEXT NOT NULL CHECK (length(trim(label)) > 0),
    body TEXT NOT NULL,
    body_sha256 TEXT NOT NULL CHECK (length(body_sha256) = 64),
    source TEXT,
    received_at TEXT NOT NULL
) STRICT;

CREATE TABLE steward_updates (
    id TEXT PRIMARY KEY,
    case_id TEXT NOT NULL REFERENCES cases(id),
    delivery_id TEXT NOT NULL REFERENCES deliveries(id),
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'applied', 'failed', 'lost')),
    base_revision INTEGER,
    base_digest TEXT,
    requester_id TEXT NOT NULL UNIQUE,
    job_id TEXT NOT NULL UNIQUE,
    request_json TEXT,
    request_sha256 TEXT,
    tool_after INTEGER NOT NULL DEFAULT 0 CHECK (tool_after >= 0),
    admitted INTEGER NOT NULL DEFAULT 0 CHECK (admitted IN (0, 1)),
    applied_revision INTEGER,
    result_posted INTEGER NOT NULL DEFAULT 0 CHECK (result_posted IN (0, 1)),
    runtime_state TEXT,
    runtime_detail TEXT,
    retry_of TEXT REFERENCES steward_updates(id),
    last_error TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    CHECK ((request_json IS NULL) = (request_sha256 IS NULL)),
    CHECK ((base_revision IS NULL) = (base_digest IS NULL)),
    CHECK (result_posted = 0 OR applied_revision IS NOT NULL),
    CHECK (runtime_state IS NULL OR applied_revision IS NOT NULL),
    CHECK (runtime_state IS NULL OR length(trim(runtime_state)) > 0),
    CHECK (runtime_state IS NOT NULL OR runtime_detail IS NULL)
) STRICT;

CREATE UNIQUE INDEX one_running_update_per_case
ON steward_updates(case_id) WHERE status = 'running';
CREATE UNIQUE INDEX one_retry_per_update
ON steward_updates(retry_of) WHERE retry_of IS NOT NULL;
CREATE INDEX steward_updates_queue
ON steward_updates(status, created_at, id);

CREATE TABLE case_revisions (
    case_id TEXT NOT NULL REFERENCES cases(id),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    markdown TEXT NOT NULL,
    markdown_sha256 TEXT NOT NULL CHECK (length(markdown_sha256) = 64),
    stage TEXT NOT NULL CHECK (stage IN ('research', 'warranted', 'contacted', 'connected', 'helped', 'closed')),
    advisory TEXT,
    summary TEXT NOT NULL,
    source_update_id TEXT UNIQUE REFERENCES steward_updates(id),
    created_at TEXT NOT NULL,
    PRIMARY KEY (case_id, revision)
) STRICT;

CREATE TABLE mailbox_receipts (
    job_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    arguments_sha256 TEXT NOT NULL CHECK (length(arguments_sha256) = 64),
    result_json TEXT NOT NULL,
    result_sha256 TEXT NOT NULL CHECK (length(result_sha256) = 64),
    is_error INTEGER NOT NULL CHECK (is_error IN (0, 1)),
    committed_revision INTEGER,
    created_at TEXT NOT NULL,
    PRIMARY KEY (job_id, call_id)
) STRICT;

CREATE INDEX case_revisions_latest
ON case_revisions(case_id, revision DESC);

CREATE TRIGGER deliveries_no_update
BEFORE UPDATE ON deliveries BEGIN
    SELECT RAISE(ABORT, 'immutable_delivery');
END;
CREATE TRIGGER deliveries_no_delete
BEFORE DELETE ON deliveries BEGIN
    SELECT RAISE(ABORT, 'immutable_delivery');
END;
CREATE TRIGGER case_revisions_no_update
BEFORE UPDATE ON case_revisions BEGIN
    SELECT RAISE(ABORT, 'immutable_revision');
END;
CREATE TRIGGER case_revisions_no_delete
BEFORE DELETE ON case_revisions BEGIN
    SELECT RAISE(ABORT, 'immutable_revision');
END;
CREATE TRIGGER mailbox_receipts_no_update
BEFORE UPDATE ON mailbox_receipts BEGIN
    SELECT RAISE(ABORT, 'immutable_receipt');
END;
CREATE TRIGGER mailbox_receipts_no_delete
BEFORE DELETE ON mailbox_receipts BEGIN
    SELECT RAISE(ABORT, 'immutable_receipt');
END;

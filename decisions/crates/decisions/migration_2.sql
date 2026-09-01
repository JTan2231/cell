BEGIN IMMEDIATE;

ALTER TABLE runs ADD COLUMN run_kind TEXT NOT NULL DEFAULT 'legacy_scan';
ALTER TABLE runs ADD COLUMN coverage_cutoff_at INTEGER;
ALTER TABLE runs ADD COLUMN observation_admission_watermark INTEGER;

CREATE TABLE product_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE observations (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    host_id TEXT,
    thread_id TEXT,
    status TEXT NOT NULL CHECK (status IN ('queued', 'processing', 'complete', 'failed')),
    scope_level INTEGER NOT NULL DEFAULT 0 CHECK (scope_level IN (0, 1)),
    attempt_epoch INTEGER NOT NULL DEFAULT 0 CHECK (attempt_epoch >= 0),
    outcome TEXT CHECK (outcome IN ('decision', 'no_decision', 'not_eligible')),
    source_digest TEXT,
    source_completed_at INTEGER,
    source_not_completed_at INTEGER,
    next_attempt_at INTEGER,
    file_change_count INTEGER NOT NULL DEFAULT 0 CHECK (file_change_count >= 0),
    authority_occurred_at INTEGER,
    failure_code TEXT,
    failure_detail TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    UNIQUE (session_id, turn_id)
);
CREATE UNIQUE INDEX observations_canonical_turn
ON observations(host_id, thread_id, turn_id)
WHERE host_id IS NOT NULL AND thread_id IS NOT NULL;
CREATE UNIQUE INDEX observations_host_turn
ON observations(host_id, turn_id)
WHERE host_id IS NOT NULL;
CREATE INDEX observations_queue ON observations(status, created_at);
CREATE INDEX observations_authority_time
ON observations(authority_occurred_at, status);

CREATE TABLE observation_jobs (
    observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
    scope_level INTEGER NOT NULL CHECK (scope_level IN (0, 1)),
    attempt INTEGER NOT NULL CHECK (attempt >= 0),
    nucleus_job_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('planned', 'submitted', 'complete', 'failed')),
    request_digest TEXT,
    admitted_at INTEGER,
    failure_detail TEXT,
    PRIMARY KEY (observation_id, scope_level, attempt)
);

CREATE TABLE observation_classification_receipts (
    nucleus_job_id TEXT NOT NULL REFERENCES observation_jobs(nucleus_job_id) ON DELETE CASCADE,
    call_id TEXT NOT NULL,
    result_json TEXT NOT NULL,
    is_error INTEGER NOT NULL CHECK (is_error IN (0, 1)),
    classification_json TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (nucleus_job_id, call_id)
);
CREATE UNIQUE INDEX observation_receipts_success
ON observation_classification_receipts(nucleus_job_id) WHERE is_error = 0;

CREATE TABLE observation_candidates (
    observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
    candidate_id TEXT NOT NULL REFERENCES candidates(id),
    PRIMARY KEY (observation_id, candidate_id)
);

CREATE TABLE observation_authority_items (
    observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    host_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    occurred_at INTEGER NOT NULL,
    timestamp_precision TEXT NOT NULL CHECK (timestamp_precision IN ('item', 'turn')),
    PRIMARY KEY (observation_id, item_id)
);
CREATE INDEX observation_authority_items_time
ON observation_authority_items(occurred_at, observation_id);

CREATE TABLE authority_verdicts (
    observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    host_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    occurred_at INTEGER NOT NULL,
    timestamp_precision TEXT NOT NULL CHECK (timestamp_precision IN ('item', 'turn')),
    verdict TEXT NOT NULL CHECK (verdict IN ('decision', 'no_decision')),
    PRIMARY KEY (observation_id, item_id)
);

INSERT INTO schema_migrations(version, applied_at)
VALUES(2, CAST(strftime('%s', 'now') AS INTEGER));
PRAGMA user_version = 2;
COMMIT;

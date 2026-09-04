PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    run_kind TEXT NOT NULL DEFAULT 'observation_projection'
        CHECK (run_kind IN ('legacy_scan', 'observation_projection')),
    report_date TEXT NOT NULL,
    window_start INTEGER NOT NULL,
    window_end INTEGER NOT NULL,
    source_manifest_hash TEXT NOT NULL,
    coverage_cutoff_at INTEGER,
    observation_admission_watermark INTEGER,
    status TEXT NOT NULL CHECK (status IN ('building', 'abandoning', 'complete', 'failed')),
    failure_code TEXT,
    failure_detail TEXT,
    content_revision INTEGER NOT NULL DEFAULT 0,
    started_at INTEGER NOT NULL,
    completed_at INTEGER
);
CREATE INDEX IF NOT EXISTS runs_report_date ON runs(report_date, started_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS runs_one_active_date
ON runs(report_date) WHERE status IN ('building', 'abandoning');

CREATE TABLE IF NOT EXISTS product_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS observations (
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
    annals_target_library_id TEXT,
    annals_target_config_path TEXT,
    failure_code TEXT,
    failure_detail TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    CHECK (
        (annals_target_library_id IS NULL AND annals_target_config_path IS NULL)
        OR
        (length(annals_target_library_id) = 32
            AND annals_target_config_path IS NOT NULL
            AND length(annals_target_config_path) > 0)
    ),
    UNIQUE (session_id, turn_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS observations_canonical_turn
ON observations(host_id, thread_id, turn_id)
WHERE host_id IS NOT NULL AND thread_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS observations_host_turn
ON observations(host_id, turn_id)
WHERE host_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS observations_queue
ON observations(status, created_at);
CREATE INDEX IF NOT EXISTS observations_authority_time
ON observations(authority_occurred_at, status);

CREATE TABLE IF NOT EXISTS run_jobs (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    nucleus_job_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('planned', 'submitted', 'complete', 'failed')),
    request_digest TEXT,
    admitted_at INTEGER,
    failure_detail TEXT,
    PRIMARY KEY (run_id, thread_id)
);

CREATE TABLE IF NOT EXISTS observation_jobs (
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

CREATE TABLE IF NOT EXISTS observation_classification_receipts (
    nucleus_job_id TEXT NOT NULL REFERENCES observation_jobs(nucleus_job_id) ON DELETE CASCADE,
    call_id TEXT NOT NULL,
    call_arguments_sha256 TEXT CHECK (
        call_arguments_sha256 IS NULL OR length(call_arguments_sha256) = 64
    ),
    result_json TEXT NOT NULL,
    is_error INTEGER NOT NULL CHECK (is_error IN (0, 1)),
    classification_json TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (nucleus_job_id, call_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS observation_receipts_success
ON observation_classification_receipts(nucleus_job_id) WHERE is_error = 0;

CREATE TABLE IF NOT EXISTS classification_receipts (
    nucleus_job_id TEXT NOT NULL REFERENCES run_jobs(nucleus_job_id) ON DELETE CASCADE,
    call_id TEXT NOT NULL,
    result_json TEXT NOT NULL,
    is_error INTEGER NOT NULL CHECK (is_error IN (0, 1)),
    classification_json TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (nucleus_job_id, call_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS classification_receipts_success
ON classification_receipts(nucleus_job_id) WHERE is_error = 0;

CREATE TABLE IF NOT EXISTS candidates (
    id TEXT PRIMARY KEY,
    decided_at INTEGER NOT NULL,
    timestamp_precision TEXT NOT NULL CHECK (timestamp_precision IN ('item', 'turn')),
    statement TEXT NOT NULL,
    disposition TEXT NOT NULL CHECK (disposition IN ('adopt', 'reject', 'forbid', 'defer', 'delegate', 'reopen', 'supersede')),
    confidence TEXT NOT NULL CHECK (confidence IN ('high', 'medium')),
    rationale TEXT,
    supersedes_id TEXT,
    authority_start INTEGER NOT NULL,
    authority_end INTEGER NOT NULL,
    review_state TEXT NOT NULL DEFAULT 'unreviewed' CHECK (review_state IN ('unreviewed', 'confirmed', 'dismissed')),
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS run_candidates (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    candidate_id TEXT NOT NULL REFERENCES candidates(id),
    PRIMARY KEY (run_id, candidate_id)
);
CREATE INDEX IF NOT EXISTS run_candidates_candidate ON run_candidates(candidate_id, run_id);

CREATE TABLE IF NOT EXISTS observation_candidates (
    observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
    candidate_id TEXT NOT NULL REFERENCES candidates(id),
    PRIMARY KEY (observation_id, candidate_id)
);

CREATE TABLE IF NOT EXISTS observation_authority_items (
    observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    host_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    occurred_at INTEGER NOT NULL,
    timestamp_precision TEXT NOT NULL CHECK (timestamp_precision IN ('item', 'turn')),
    PRIMARY KEY (observation_id, item_id)
);
CREATE INDEX IF NOT EXISTS observation_authority_items_time
ON observation_authority_items(occurred_at, observation_id);

CREATE TABLE IF NOT EXISTS authority_verdicts (
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

CREATE TABLE IF NOT EXISTS candidate_sources (
    candidate_id TEXT NOT NULL REFERENCES candidates(id) ON DELETE CASCADE,
    source_role TEXT NOT NULL CHECK (source_role IN ('authority', 'context')),
    host_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    message_role TEXT NOT NULL CHECK (message_role IN ('user', 'assistant')),
    occurred_at INTEGER NOT NULL,
    timestamp_precision TEXT NOT NULL CHECK (timestamp_precision IN ('item', 'turn')),
    PRIMARY KEY (candidate_id, source_role, item_id)
);

CREATE TABLE IF NOT EXISTS reviews (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    candidate_id TEXT NOT NULL REFERENCES candidates(id) ON DELETE CASCADE,
    action TEXT NOT NULL CHECK (action IN ('confirm', 'dismiss')),
    reviewed_at INTEGER NOT NULL,
    review_source TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS decision_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    envelope_version INTEGER NOT NULL CHECK (envelope_version = 1),
    event_kind TEXT NOT NULL CHECK (event_kind IN ('decision_admitted', 'decision_reviewed')),
    decision_id TEXT NOT NULL REFERENCES candidates(id),
    review_id INTEGER REFERENCES reviews(id),
    occurred_at INTEGER NOT NULL,
    envelope_json TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS decision_events_one_admission
ON decision_events(decision_id) WHERE event_kind='decision_admitted';
CREATE UNIQUE INDEX IF NOT EXISTS decision_events_one_review
ON decision_events(review_id) WHERE event_kind='decision_reviewed';

CREATE TABLE IF NOT EXISTS digest_snapshots (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    content_revision INTEGER NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    digest_hash TEXT NOT NULL,
    frozen_at INTEGER NOT NULL,
    PRIMARY KEY (run_id, content_revision)
);

CREATE TABLE IF NOT EXISTS deliveries (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id),
    content_revision INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('manual', 'scheduled')),
    occurrence_date TEXT,
    idempotency_key TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('pending', 'accepted', 'failed')),
    email_id TEXT,
    failure_detail TEXT,
    created_at INTEGER NOT NULL,
    completed_at INTEGER,
    UNIQUE (kind, occurrence_date)
);
CREATE UNIQUE INDEX IF NOT EXISTS deliveries_one_open_manual
ON deliveries(run_id, content_revision)
WHERE kind='manual' AND status IN ('pending', 'failed');

CREATE TABLE IF NOT EXISTS decision_accounts (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    occurred_at INTEGER NOT NULL,
    timestamp_precision TEXT NOT NULL CHECK (timestamp_precision IN ('item', 'turn')),
    statement TEXT,
    authority_quote TEXT,
    context TEXT,
    action TEXT,
    result TEXT,
    authority_start INTEGER NOT NULL CHECK (authority_start >= 0),
    authority_end INTEGER NOT NULL CHECK (authority_end > authority_start),
    capture_rule_version TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    CHECK (
        (statement IS NOT NULL AND authority_quote IS NOT NULL)
        OR
        (statement IS NULL AND authority_quote IS NULL AND context IS NULL
            AND action IS NULL AND result IS NULL)
    )
);

CREATE TABLE IF NOT EXISTS decision_account_sources (
    account_id TEXT NOT NULL REFERENCES decision_accounts(id) ON DELETE CASCADE,
    source_role TEXT NOT NULL CHECK (source_role IN ('authority', 'context', 'action', 'result')),
    source_order INTEGER NOT NULL CHECK (source_order >= 0),
    host_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    message_role TEXT NOT NULL CHECK (message_role IN ('user', 'assistant')),
    occurred_at INTEGER NOT NULL,
    timestamp_precision TEXT NOT NULL CHECK (timestamp_precision IN ('item', 'turn')),
    PRIMARY KEY (account_id, source_role, source_order),
    UNIQUE (account_id, source_role, item_id)
);

CREATE TABLE IF NOT EXISTS observation_accounts (
    observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES decision_accounts(id),
    PRIMARY KEY (observation_id, account_id)
);

CREATE TABLE IF NOT EXISTS decision_account_outbox (
    account_id TEXT PRIMARY KEY REFERENCES decision_accounts(id),
    producer TEXT NOT NULL CHECK (producer = 'krisis'),
    producer_key TEXT NOT NULL UNIQUE,
    account_markdown TEXT,
    source_sha256 TEXT NOT NULL,
    target_library_id TEXT NOT NULL CHECK (length(target_library_id) = 32),
    target_config_path TEXT NOT NULL CHECK (length(target_config_path) > 0),
    status TEXT NOT NULL CHECK (status IN ('pending', 'accepted')),
    annals_contract_version INTEGER,
    annals_library_id TEXT,
    annals_job_id TEXT,
    annals_accepted_at TEXT,
    annals_acceptance TEXT CHECK (
        annals_acceptance IS NULL OR annals_acceptance IN ('created', 'replayed')
    ),
    created_at INTEGER NOT NULL,
    accepted_at INTEGER,
    CHECK (
        (status='pending' AND account_markdown IS NOT NULL
            AND annals_contract_version IS NULL AND annals_library_id IS NULL
            AND annals_job_id IS NULL AND annals_accepted_at IS NULL
            AND annals_acceptance IS NULL
            AND accepted_at IS NULL)
        OR
        (status='accepted' AND account_markdown IS NULL
            AND annals_contract_version IS NOT NULL AND annals_library_id IS NOT NULL
            AND annals_job_id IS NOT NULL AND annals_accepted_at IS NOT NULL
            AND annals_acceptance IN ('created', 'replayed')
            AND accepted_at IS NOT NULL)
    )
);
CREATE INDEX IF NOT EXISTS decision_account_outbox_pending
ON decision_account_outbox(status, created_at, account_id);

PRAGMA user_version = 4;

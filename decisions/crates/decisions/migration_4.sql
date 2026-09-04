BEGIN IMMEDIATE;

ALTER TABLE observation_classification_receipts
ADD COLUMN call_arguments_sha256 TEXT CHECK (
    call_arguments_sha256 IS NULL OR length(call_arguments_sha256) = 64
);

ALTER TABLE observations ADD COLUMN annals_target_library_id TEXT;
ALTER TABLE observations ADD COLUMN annals_target_config_path TEXT;

CREATE TABLE decision_accounts (
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

CREATE TABLE decision_account_sources (
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

CREATE TABLE observation_accounts (
    observation_id TEXT NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES decision_accounts(id),
    PRIMARY KEY (observation_id, account_id)
);

CREATE TABLE decision_account_outbox (
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
CREATE INDEX decision_account_outbox_pending
ON decision_account_outbox(status, created_at, account_id);

INSERT INTO schema_migrations(version, applied_at)
VALUES(4, CAST(strftime('%s', 'now') AS INTEGER));
PRAGMA user_version = 4;
COMMIT;

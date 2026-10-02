-- Every pre-version-5 library is a general library. Decisions libraries are
-- created explicitly at version 5 and cannot be obtained by migration.
CREATE TABLE library_profile (
    singleton  INTEGER PRIMARY KEY CHECK (singleton = 1),
    kind       TEXT NOT NULL CHECK (kind IN ('general', 'decisions'))
);

INSERT INTO library_profile(singleton, kind) VALUES (1, 'general');

CREATE TRIGGER library_profile_immutable_update
BEFORE UPDATE ON library_profile BEGIN
    SELECT RAISE(ABORT, 'library profile is immutable');
END;

CREATE TRIGGER library_profile_immutable_delete
BEFORE DELETE ON library_profile BEGIN
    SELECT RAISE(ABORT, 'library profile is immutable');
END;

-- Producer acceptance is library-scoped because every row lives in exactly
-- one Annals database. The immutable projection is the accepted-account feed.
CREATE TABLE decision_account_acceptances (
    sequence                 INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id                 TEXT NOT NULL UNIQUE
                                  CHECK (length(event_id) = 36
                                         AND event_id GLOB 'dae_[0-9a-f]*'),
    producer                 TEXT NOT NULL CHECK (producer = 'krisis'),
    producer_key             TEXT NOT NULL CHECK (length(trim(producer_key)) > 0),
    source_sha256            TEXT NOT NULL
                                  CHECK (length(source_sha256) = 64
                                         AND source_sha256 = lower(source_sha256)
                                         AND source_sha256 NOT GLOB '*[^0-9a-f]*'),
    job_id                   TEXT NOT NULL UNIQUE CHECK (length(trim(job_id)) > 0),
    accepted_at              TEXT NOT NULL CHECK (length(trim(accepted_at)) > 0),
    account_schema_version   INTEGER NOT NULL CHECK (account_schema_version = 1),
    statement                TEXT NOT NULL CHECK (length(trim(statement)) > 0),
    context                  TEXT NOT NULL CHECK (length(trim(context)) > 0),
    action                   TEXT NOT NULL CHECK (length(trim(action)) > 0),
    result                   TEXT NOT NULL CHECK (length(trim(result)) > 0),
    occurred_at              INTEGER NOT NULL,
    occurred_at_precision    TEXT NOT NULL CHECK (length(trim(occurred_at_precision)) > 0),
    capture_rule_version     TEXT NOT NULL CHECK (length(trim(capture_rule_version)) > 0),
    authority_host_id        TEXT NOT NULL CHECK (length(trim(authority_host_id)) > 0),
    authority_thread_id      TEXT NOT NULL CHECK (length(trim(authority_thread_id)) > 0),
    authority_turn_id        TEXT NOT NULL CHECK (length(trim(authority_turn_id)) > 0),
    authority_item_id        TEXT NOT NULL CHECK (length(trim(authority_item_id)) > 0),
    authority_span_start     INTEGER NOT NULL CHECK (authority_span_start >= 0),
    authority_span_end       INTEGER NOT NULL CHECK (authority_span_end > authority_span_start),
    UNIQUE(producer, producer_key)
);

CREATE TRIGGER decision_account_acceptances_immutable_update
BEFORE UPDATE ON decision_account_acceptances BEGIN
    SELECT RAISE(ABORT, 'decision account acceptances are immutable');
END;

CREATE TRIGGER decision_account_acceptances_immutable_delete
BEFORE DELETE ON decision_account_acceptances BEGIN
    SELECT RAISE(ABORT, 'decision account acceptances are immutable');
END;

PRAGMA user_version = 5;

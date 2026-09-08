-- Preserve historical account projections and exact feed positions.
ALTER TABLE decision_account_acceptances RENAME TO legacy_decision_account_acceptances;
DROP TRIGGER decision_account_acceptances_immutable_update;
DROP TRIGGER decision_account_acceptances_immutable_delete;
CREATE TRIGGER legacy_decision_account_acceptances_immutable_update
BEFORE UPDATE ON legacy_decision_account_acceptances BEGIN
    SELECT RAISE(ABORT, 'historical account acceptances are immutable');
END;
CREATE TRIGGER legacy_decision_account_acceptances_immutable_delete
BEFORE DELETE ON legacy_decision_account_acceptances BEGIN
    SELECT RAISE(ABORT, 'historical account acceptances are immutable');
END;
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
    UNIQUE(producer, producer_key)
);


INSERT INTO decision_account_acceptances
(sequence, event_id, producer, producer_key, source_sha256, job_id, accepted_at)
SELECT sequence, event_id, producer, producer_key, source_sha256, job_id, accepted_at
FROM legacy_decision_account_acceptances ORDER BY sequence;
CREATE TRIGGER decision_account_acceptances_immutable_update
BEFORE UPDATE ON decision_account_acceptances BEGIN
    SELECT RAISE(ABORT, 'document acceptances are immutable');
END;
CREATE TRIGGER decision_account_acceptances_immutable_delete
BEFORE DELETE ON decision_account_acceptances BEGIN
    SELECT RAISE(ABORT, 'document acceptances are immutable');
END;
PRAGMA user_version = 7;

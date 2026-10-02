-- Version 2 deliberately discards the mixed raw log and historical mailbox.
-- A pending call with nonterminal owners is live execution authority, so
-- refuse the cutover until those requester calls have settled. Terminal-owner
-- pending rows are stale history and are discarded below.
CREATE TABLE migration_v1_to_v2_guard (
    pending_calls INTEGER NOT NULL CHECK (pending_calls = 0)
) STRICT;

INSERT INTO migration_v1_to_v2_guard (pending_calls)
SELECT COUNT(*)
FROM pending_tool_calls AS calls
JOIN jobs ON jobs.id = calls.job_id
JOIN attempts ON attempts.job_id = calls.job_id AND attempts.id = calls.attempt_id
WHERE calls.state = 'pending'
  AND jobs.state IN ('accepted', 'running', 'waiting_on_requester')
  AND attempts.state IN ('pending', 'starting', 'running', 'waiting_on_requester');

DROP TABLE migration_v1_to_v2_guard;
DROP TABLE pending_tool_calls;
DROP TABLE log_records;
DELETE FROM log_schemas WHERE id = 'nucleus.lifecycle-event.v1';
ALTER TABLE jobs DROP COLUMN next_log_sequence;

CREATE TABLE harness_output_records (
    attempt_id   TEXT NOT NULL REFERENCES attempts(id),
    sequence     INTEGER NOT NULL CHECK (sequence > 0),
    observed_at  TEXT NOT NULL,
    payload      BLOB NOT NULL,
    PRIMARY KEY (attempt_id, sequence)
) STRICT;

CREATE TABLE pending_tool_calls (
    job_id               TEXT NOT NULL REFERENCES jobs(id),
    id                   TEXT NOT NULL,
    attempt_id           TEXT NOT NULL,
    state                TEXT NOT NULL CHECK (state IN ('pending', 'answered')),
    tool_name            TEXT NOT NULL,
    arguments_schema_id  TEXT NOT NULL REFERENCES log_schemas(id),
    arguments_bytes      BLOB NOT NULL,
    arguments_digest     BLOB NOT NULL CHECK (length(arguments_digest) = 32),
    request_sequence     INTEGER NOT NULL,
    result_schema_id     TEXT REFERENCES log_schemas(id),
    result_bytes         BLOB,
    result_digest        BLOB CHECK (result_digest IS NULL OR length(result_digest) = 32),
    result_is_error      INTEGER CHECK (result_is_error IN (0, 1)),
    created_at           TEXT NOT NULL,
    answered_at          TEXT,
    PRIMARY KEY (job_id, id),
    FOREIGN KEY (job_id, attempt_id) REFERENCES attempts(job_id, id),
    FOREIGN KEY (attempt_id, request_sequence)
        REFERENCES harness_output_records(attempt_id, sequence),
    CHECK (
        (state = 'pending'
            AND result_schema_id IS NULL
            AND result_bytes IS NULL
            AND result_digest IS NULL
            AND result_is_error IS NULL
            AND answered_at IS NULL)
        OR
        (state = 'answered'
            AND result_schema_id IS NOT NULL
            AND result_bytes IS NOT NULL
            AND result_digest IS NOT NULL
            AND result_is_error IS NOT NULL
            AND answered_at IS NOT NULL)
    )
) STRICT;

CREATE INDEX pending_tool_calls_mailbox
    ON pending_tool_calls(job_id, state, request_sequence);

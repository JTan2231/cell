CREATE TABLE log_schemas (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    version           TEXT NOT NULL,
    media_type        TEXT NOT NULL,
    producer          TEXT NOT NULL,
    producer_version  TEXT,
    schema_bytes      BLOB NOT NULL,
    schema_digest     BLOB NOT NULL CHECK (length(schema_digest) = 32),
    created_at        TEXT NOT NULL
) STRICT;

CREATE TABLE jobs (
    id                         TEXT PRIMARY KEY,
    label                      TEXT NOT NULL,
    requester_program          TEXT NOT NULL,
    requester_id               TEXT NOT NULL,
    parent_job_id              TEXT REFERENCES jobs(id),
    request_schema_id          TEXT NOT NULL REFERENCES log_schemas(id),
    request_bytes              BLOB NOT NULL,
    request_digest             BLOB NOT NULL CHECK (length(request_digest) = 32),
    state                      TEXT NOT NULL CHECK (
        state IN (
            'accepted',
            'running',
            'waiting_on_requester',
            'completed',
            'failed',
            'cancelled'
        )
    ),
    current_attempt_id         TEXT,
    cancellation_requested_at  TEXT,
    created_at                 TEXT NOT NULL,
    updated_at                 TEXT NOT NULL,
    completed_at               TEXT,
    terminal_reason            TEXT
) STRICT;

CREATE INDEX jobs_by_requester
    ON jobs(requester_program, requester_id, created_at, id);

CREATE INDEX jobs_by_parent
    ON jobs(parent_job_id, created_at, id);

CREATE INDEX jobs_by_state
    ON jobs(state, created_at, id);

CREATE TABLE attempts (
    id                TEXT PRIMARY KEY,
    job_id            TEXT NOT NULL REFERENCES jobs(id),
    ordinal           INTEGER NOT NULL CHECK (ordinal > 0),
    harness           TEXT NOT NULL,
    harness_version   TEXT NOT NULL,
    adapter_version   TEXT NOT NULL,
    state             TEXT NOT NULL CHECK (
        state IN (
            'pending',
            'starting',
            'running',
            'waiting_on_requester',
            'completed',
            'failed',
            'cancelled',
            'timed_out',
            'lost'
        )
    ),
    process_id        INTEGER,
    process_group_id  INTEGER,
    created_at        TEXT NOT NULL,
    started_at        TEXT,
    completed_at      TEXT,
    terminal_reason   TEXT,
    terminal_message  TEXT,
    UNIQUE (job_id, ordinal),
    UNIQUE (job_id, id)
) STRICT;

CREATE INDEX attempts_by_job
    ON attempts(job_id, ordinal);

CREATE INDEX attempts_by_state
    ON attempts(state, created_at, id);

CREATE TABLE toolsets (
    provider                TEXT NOT NULL,
    name                    TEXT NOT NULL,
    version                 INTEGER NOT NULL CHECK (version > 0),
    definitions_schema_id   TEXT NOT NULL REFERENCES log_schemas(id),
    definitions_bytes       BLOB NOT NULL,
    definitions_digest      BLOB NOT NULL CHECK (length(definitions_digest) = 32),
    created_at              TEXT NOT NULL,
    PRIMARY KEY (provider, name, version)
) STRICT;

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

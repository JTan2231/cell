DROP INDEX tool_calls_one_successful_submission;

CREATE TABLE reconciliation_drafts (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    model_run_id       INTEGER NOT NULL REFERENCES model_runs(id) ON DELETE CASCADE,
    work_id            INTEGER NOT NULL REFERENCES works(id) ON DELETE RESTRICT,
    base_revision      INTEGER NOT NULL CHECK (base_revision >= 0),
    status             TEXT NOT NULL
                           CHECK (status IN ('open', 'finalized', 'discarded', 'abandoned')),
    version            INTEGER NOT NULL CHECK (version >= 1),
    summary            TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    annotations        TEXT NOT NULL
                           CHECK (json_valid(annotations) AND json_type(annotations) = 'array'),
    created_sequence   INTEGER NOT NULL CHECK (created_sequence >= 0),
    terminal_sequence  INTEGER CHECK (terminal_sequence >= created_sequence),
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    completed_at       TEXT,
    FOREIGN KEY(model_run_id, created_sequence)
        REFERENCES tool_calls(model_run_id, sequence) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(model_run_id, terminal_sequence)
        REFERENCES tool_calls(model_run_id, sequence) DEFERRABLE INITIALLY DEFERRED,
    CHECK (
        (status = 'open' AND terminal_sequence IS NULL AND completed_at IS NULL)
        OR (
            status IN ('finalized', 'discarded')
            AND terminal_sequence IS NOT NULL
            AND completed_at IS NOT NULL
        )
        OR (status = 'abandoned' AND terminal_sequence IS NULL AND completed_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX reconciliation_drafts_one_open_per_model_run
    ON reconciliation_drafts(model_run_id)
    WHERE status = 'open';

CREATE UNIQUE INDEX reconciliation_drafts_one_finalized_per_model_run
    ON reconciliation_drafts(model_run_id)
    WHERE status = 'finalized';

CREATE TABLE reconciliation_draft_operations (
    draft_id             INTEGER NOT NULL
                             REFERENCES reconciliation_drafts(id) ON DELETE CASCADE,
    slot                 INTEGER NOT NULL CHECK (slot > 0),
    ordinal              INTEGER NOT NULL CHECK (ordinal >= 0),
    operation            TEXT NOT NULL CHECK (json_valid(operation)),
    status               TEXT NOT NULL
                             CHECK (status IN (
                                 'staged', 'needs_revision', 'blocked', 'implicated', 'dropped'
                             )),
    hint                 TEXT,
    created_version      INTEGER NOT NULL CHECK (created_version >= 1),
    last_changed_version INTEGER NOT NULL CHECK (last_changed_version >= created_version),
    PRIMARY KEY(draft_id, slot),
    UNIQUE(draft_id, ordinal)
) WITHOUT ROWID;

ALTER TABLE reconciliations
    ADD COLUMN reconciliation_draft_id INTEGER
        REFERENCES reconciliation_drafts(id) ON DELETE RESTRICT;

CREATE UNIQUE INDEX reconciliations_one_per_draft
    ON reconciliations(reconciliation_draft_id)
    WHERE reconciliation_draft_id IS NOT NULL;

-- Preserve the provenance of model reconciliations recorded under the pre-draft contract.
INSERT INTO reconciliation_drafts(
    model_run_id, work_id, base_revision, status, version, summary, annotations,
    created_sequence, terminal_sequence, created_at, updated_at, completed_at
)
SELECT
    r.model_run_id,
    r.work_id,
    r.base_revision,
    'finalized',
    1,
    r.summary,
    json(COALESCE(json_extract(r.submitted_request, '$.annotations'), '[]')),
    t.sequence,
    t.sequence,
    r.created_at,
    r.created_at,
    COALESCE(m.completed_at, r.created_at)
FROM reconciliations AS r
JOIN model_runs AS m ON m.id = r.model_run_id
JOIN tool_calls AS t
  ON t.model_run_id = r.model_run_id
 AND t.tool_name = 'submit_reconciliation'
 AND t.succeeded = 1
WHERE r.model_run_id IS NOT NULL;

INSERT INTO reconciliation_draft_operations(
    draft_id, slot, ordinal, operation, status, hint, created_version, last_changed_version
)
SELECT
    d.id,
    CAST(operation.key AS INTEGER) + 1,
    CAST(operation.key AS INTEGER),
    json(operation.value),
    'staged',
    NULL,
    1,
    1
FROM reconciliation_drafts AS d
JOIN reconciliations AS r ON r.model_run_id = d.model_run_id
JOIN json_each(r.submitted_request, '$.operations') AS operation
WHERE d.status = 'finalized';

UPDATE reconciliations
SET reconciliation_draft_id = (
    SELECT d.id
    FROM reconciliation_drafts AS d
    WHERE d.model_run_id = reconciliations.model_run_id
      AND d.status = 'finalized'
)
WHERE model_run_id IS NOT NULL;

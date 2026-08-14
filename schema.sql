CREATE TABLE library_state (
    singleton   INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision    INTEGER NOT NULL CHECK (revision >= 0),
    library_id  TEXT NOT NULL UNIQUE
                    CHECK (
                        length(library_id) = 32
                        AND library_id = lower(library_id)
                        AND library_id NOT GLOB '*[^0-9a-f]*'
                    )
);

INSERT INTO library_state(singleton, revision, library_id)
VALUES (1, 0, lower(hex(randomblob(16))));

CREATE TABLE works (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    label             TEXT NOT NULL CHECK (length(trim(label)) > 0),
    normalized_label  TEXT NOT NULL UNIQUE,
    text              TEXT NOT NULL CHECK (length(text) > 0),
    sha256            TEXT NOT NULL UNIQUE
                          CHECK (
                              length(sha256) = 64
                              AND sha256 = lower(sha256)
                              AND sha256 NOT GLOB '*[^0-9a-f]*'
                          ),
    created_at        TEXT NOT NULL
);

CREATE TABLE concepts (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    label  TEXT NOT NULL CHECK (length(trim(label)) > 0),
    CHECK (id > 0)
);

CREATE TABLE concept_edges (
    parent_id  INTEGER NOT NULL
                   REFERENCES concepts(id)
                   ON DELETE CASCADE
                   DEFERRABLE INITIALLY DEFERRED,
    child_id   INTEGER NOT NULL
                   REFERENCES concepts(id)
                   ON DELETE CASCADE
                   DEFERRABLE INITIALLY DEFERRED,
    CHECK (parent_id <> child_id),
    PRIMARY KEY(parent_id, child_id)
) WITHOUT ROWID;

CREATE INDEX concept_edges_by_child
    ON concept_edges(child_id, parent_id);

CREATE TABLE evidence (
    concept_id  INTEGER NOT NULL
                    REFERENCES concepts(id) ON DELETE CASCADE,
    work_id     INTEGER NOT NULL
                    REFERENCES works(id) ON DELETE RESTRICT,
    start_byte  INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte    INTEGER NOT NULL CHECK (end_byte > start_byte),
    PRIMARY KEY(concept_id, work_id, start_byte, end_byte)
) WITHOUT ROWID;

CREATE INDEX evidence_by_work_range
    ON evidence(work_id, start_byte, end_byte, concept_id);

CREATE TABLE model_runs (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    token            TEXT NOT NULL UNIQUE,
    work_id          INTEGER NOT NULL REFERENCES works(id) ON DELETE RESTRICT,
    base_revision    INTEGER NOT NULL CHECK (base_revision >= 0),
    status           TEXT NOT NULL
                         CHECK (status IN ('running', 'submitted', 'no_submission', 'failed')),
    model            TEXT NOT NULL,
    reasoning_effort TEXT NOT NULL,
    prompt_version   TEXT NOT NULL,
    final_response   TEXT,
    failure          TEXT,
    created_at       TEXT NOT NULL,
    completed_at     TEXT
);

CREATE UNIQUE INDEX model_runs_one_active_context
    ON model_runs(work_id, base_revision, model, reasoning_effort, prompt_version)
    WHERE status = 'running';

CREATE TABLE tool_calls (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    model_run_id INTEGER NOT NULL REFERENCES model_runs(id) ON DELETE CASCADE,
    sequence     INTEGER NOT NULL CHECK (sequence >= 0),
    tool_name    TEXT NOT NULL,
    arguments    TEXT NOT NULL CHECK (json_valid(arguments)),
    result       TEXT NOT NULL CHECK (json_valid(result)),
    succeeded    INTEGER NOT NULL CHECK (succeeded IN (0, 1)),
    created_at   TEXT NOT NULL,
    UNIQUE(model_run_id, sequence)
);

CREATE UNIQUE INDEX tool_calls_one_successful_submission
    ON tool_calls(model_run_id)
    WHERE tool_name = 'submit_reconciliation' AND succeeded = 1;

CREATE TABLE reconciliations (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    work_id                  INTEGER NOT NULL REFERENCES works(id) ON DELETE RESTRICT,
    base_revision            INTEGER NOT NULL CHECK (base_revision >= 0),
    model_run_id             INTEGER REFERENCES model_runs(id) ON DELETE RESTRICT,
    status                   TEXT NOT NULL
                                 CHECK (status IN ('pending', 'applied', 'superseded', 'recorded')),
    summary                  TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    submitted_request        TEXT NOT NULL CHECK (json_valid(submitted_request)),
    resolved_reconciliation  TEXT NOT NULL CHECK (json_valid(resolved_reconciliation)),
    actor                    TEXT NOT NULL,
    created_at               TEXT NOT NULL,
    applied_revision         INTEGER REFERENCES commits(revision) ON DELETE RESTRICT,
    CHECK (
        (status = 'applied' AND applied_revision IS NOT NULL)
        OR (status <> 'applied' AND applied_revision IS NULL)
    )
);

CREATE INDEX reconciliations_by_work_status
    ON reconciliations(work_id, status, id DESC);

CREATE UNIQUE INDEX reconciliations_one_pending_per_work
    ON reconciliations(work_id)
    WHERE status = 'pending';

CREATE UNIQUE INDEX reconciliations_one_per_model_run
    ON reconciliations(model_run_id)
    WHERE model_run_id IS NOT NULL;

CREATE TABLE commits (
    revision             INTEGER PRIMARY KEY CHECK (revision > 0),
    parent_revision      INTEGER NOT NULL CHECK (parent_revision >= 0),
    base_revision        INTEGER NOT NULL CHECK (base_revision >= 0),
    work_id              INTEGER REFERENCES works(id) ON DELETE RESTRICT,
    reconciliation_id    INTEGER REFERENCES reconciliations(id) ON DELETE RESTRICT,
    kind                 TEXT NOT NULL CHECK (kind IN ('change', 'revert', 'shake')),
    summary              TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    submitted_request    TEXT NOT NULL CHECK (json_valid(submitted_request)),
    resolved_operations  TEXT NOT NULL CHECK (json_valid(resolved_operations)),
    after_snapshot       TEXT NOT NULL CHECK (json_valid(after_snapshot)),
    metadata             TEXT NOT NULL CHECK (json_valid(metadata)),
    actor                TEXT NOT NULL,
    created_at           TEXT NOT NULL,
    CHECK (parent_revision = revision - 1),
    CHECK (base_revision = parent_revision),
    CHECK (
        (kind = 'change' AND work_id IS NOT NULL AND reconciliation_id IS NOT NULL)
        OR (kind = 'revert' AND reconciliation_id IS NULL)
        OR (kind = 'shake' AND work_id IS NULL AND reconciliation_id IS NULL)
    )
);

CREATE UNIQUE INDEX commits_one_per_reconciliation
    ON commits(reconciliation_id)
    WHERE reconciliation_id IS NOT NULL;

CREATE TABLE concept_search (
    concept_id            INTEGER PRIMARY KEY REFERENCES concepts(id) ON DELETE CASCADE,
    label                 TEXT NOT NULL,
    ancestors             TEXT NOT NULL,
    normalized_label      TEXT NOT NULL,
    normalized_ancestors  TEXT NOT NULL,
    content_hash          TEXT NOT NULL,
    indexer_version       INTEGER NOT NULL CHECK (indexer_version > 0)
);

CREATE VIRTUAL TABLE concept_fts USING fts5(
    label,
    ancestors,
    content = 'concept_search',
    content_rowid = 'concept_id',
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3 4'
);

CREATE TRIGGER concept_search_after_insert
AFTER INSERT ON concept_search
BEGIN
    INSERT INTO concept_fts(rowid, label, ancestors)
    VALUES (new.concept_id, new.label, new.ancestors);
END;

CREATE TRIGGER concept_search_after_delete
AFTER DELETE ON concept_search
BEGIN
    INSERT INTO concept_fts(concept_fts, rowid, label, ancestors)
    VALUES ('delete', old.concept_id, old.label, old.ancestors);
END;

CREATE TRIGGER concept_search_after_update
AFTER UPDATE ON concept_search
BEGIN
    INSERT INTO concept_fts(concept_fts, rowid, label, ancestors)
    VALUES ('delete', old.concept_id, old.label, old.ancestors);
    INSERT INTO concept_fts(rowid, label, ancestors)
    VALUES (new.concept_id, new.label, new.ancestors);
END;

CREATE TABLE index_metadata (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);

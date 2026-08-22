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

CREATE TRIGGER works_immutable_update
BEFORE UPDATE ON works BEGIN
    SELECT RAISE(ABORT, 'works are immutable');
END;

CREATE TRIGGER works_immutable_delete
BEFORE DELETE ON works BEGIN
    SELECT RAISE(ABORT, 'works are immutable');
END;

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
    end_byte    INTEGER NOT NULL
                    CHECK (end_byte > start_byte AND end_byte - start_byte <= 8192),
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
    model_run_id INTEGER NOT NULL REFERENCES model_runs(id) ON DELETE CASCADE,
    sequence     INTEGER NOT NULL CHECK (sequence >= 0),
    tool_name    TEXT NOT NULL,
    arguments    TEXT NOT NULL CHECK (json_valid(arguments)),
    result       TEXT NOT NULL CHECK (json_valid(result)),
    succeeded    INTEGER NOT NULL CHECK (succeeded IN (0, 1)),
    created_at   TEXT NOT NULL,
    PRIMARY KEY(model_run_id, sequence)
) WITHOUT ROWID;

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
    work_id              INTEGER REFERENCES works(id) ON DELETE RESTRICT,
    reconciliation_id    INTEGER REFERENCES reconciliations(id) ON DELETE RESTRICT,
    kind                 TEXT NOT NULL CHECK (kind IN ('change', 'revert', 'shake')),
    summary              TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    submitted_request    TEXT NOT NULL CHECK (json_valid(submitted_request)),
    resolved_operations  TEXT NOT NULL CHECK (json_valid(resolved_operations)),
    after_snapshot       TEXT NOT NULL CHECK (json_valid(after_snapshot)),
    actor                TEXT NOT NULL,
    created_at           TEXT NOT NULL,
    CHECK (
        (kind = 'change' AND work_id IS NOT NULL AND reconciliation_id IS NOT NULL)
        OR (kind = 'revert' AND reconciliation_id IS NULL)
        OR (kind = 'shake' AND work_id IS NULL AND reconciliation_id IS NULL)
    )
);

CREATE UNIQUE INDEX commits_one_per_reconciliation
    ON commits(reconciliation_id)
    WHERE reconciliation_id IS NOT NULL;

-- One row per source delivery. Works remain content-addressed and may be
-- selected by several deliveries; this receipt preserves each delivery's
-- captured file metadata and Annals lifecycle independently.
CREATE TABLE ingestions (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    delivery_key          TEXT UNIQUE CHECK (
                              delivery_key IS NULL OR length(trim(delivery_key)) > 0
                          ),
    source_name           TEXT NOT NULL CHECK (length(source_name) > 0),
    channel               TEXT NOT NULL
                              CHECK (channel IN ('manual', 'inbox')),
    source_size_bytes     INTEGER CHECK (source_size_bytes >= 0),
    source_created_at     TEXT CHECK (
                              source_created_at IS NULL
                              OR length(trim(source_created_at)) > 0
                          ),
    source_modified_at    TEXT CHECK (
                              source_modified_at IS NULL
                              OR length(trim(source_modified_at)) > 0
                          ),
    first_seen_at         TEXT NOT NULL CHECK (length(trim(first_seen_at)) > 0),
    ingested_at           TEXT CHECK (
                              ingested_at IS NULL OR length(trim(ingested_at)) > 0
                          ),
    completed_at          TEXT CHECK (
                              completed_at IS NULL OR length(trim(completed_at)) > 0
                          ),
    status                TEXT NOT NULL
                              CHECK (status IN ('processing', 'completed', 'failed')),
    work_id               INTEGER REFERENCES works(id) ON DELETE RESTRICT,
    new_work              INTEGER CHECK (new_work IN (0, 1)),
    result                TEXT
                              CHECK (result IN ('retained', 'pending', 'applied', 'recorded')),
    result_revision       INTEGER REFERENCES commits(revision) ON DELETE RESTRICT,
    error_code            TEXT CHECK (
                              error_code IS NULL OR length(trim(error_code)) > 0
                          ),
    error_message         TEXT CHECK (
                              error_message IS NULL OR length(trim(error_message)) > 0
                          ),
    CHECK (
        (channel = 'inbox' AND delivery_key IS NOT NULL)
        OR (channel = 'manual' AND delivery_key IS NULL)
    ),
    CHECK (
        (work_id IS NULL AND new_work IS NULL AND ingested_at IS NULL)
        OR (work_id IS NOT NULL AND new_work IS NOT NULL AND ingested_at IS NOT NULL)
    ),
    CHECK (
        (error_code IS NULL AND error_message IS NULL)
        OR (error_code IS NOT NULL AND error_message IS NOT NULL)
    ),
    CHECK (
        (status = 'processing' AND completed_at IS NULL AND result IS NULL)
        OR (
            status = 'completed' AND completed_at IS NOT NULL AND result IS NOT NULL
            AND work_id IS NOT NULL AND error_code IS NULL AND error_message IS NULL
        )
        OR (
            status = 'failed' AND completed_at IS NOT NULL AND result IS NULL
            AND error_code IS NOT NULL AND error_message IS NOT NULL
        )
    ),
    CHECK (
        (result = 'applied' AND result_revision IS NOT NULL)
        OR ((result IS NULL OR result <> 'applied') AND result_revision IS NULL)
    )
);

CREATE INDEX ingestions_by_created
    ON ingestions(source_created_at DESC, id DESC);
CREATE INDEX ingestions_by_modified
    ON ingestions(source_modified_at DESC, id DESC);
CREATE INDEX ingestions_by_first_seen
    ON ingestions(first_seen_at DESC, id DESC);
CREATE INDEX ingestions_by_ingested
    ON ingestions(ingested_at DESC, id DESC);
CREATE INDEX ingestions_by_completed
    ON ingestions(completed_at DESC, id DESC);

CREATE UNIQUE INDEX ingestions_one_new_work_per_work
    ON ingestions(work_id)
    WHERE new_work = 1;

-- Queryable, immutable full snapshots of committed corpus revisions. Revision
-- zero remains the implicit empty corpus and therefore has no stored row.
CREATE TABLE revision_snapshots (
    revision        INTEGER PRIMARY KEY
                        REFERENCES commits(revision) ON DELETE RESTRICT,
    concept_count   INTEGER NOT NULL CHECK (concept_count >= 0),
    edge_count      INTEGER NOT NULL CHECK (edge_count >= 0),
    evidence_count  INTEGER NOT NULL CHECK (evidence_count >= 0),
    CHECK (revision > 0)
);

CREATE TABLE revision_concepts (
    revision         INTEGER NOT NULL
                         REFERENCES revision_snapshots(revision)
                         ON DELETE RESTRICT
                         DEFERRABLE INITIALLY DEFERRED,
    concept_id       INTEGER NOT NULL CHECK (concept_id > 0),
    label            TEXT NOT NULL CHECK (length(trim(label)) > 0),
    normalized_label TEXT NOT NULL,
    parent_count     INTEGER NOT NULL CHECK (parent_count >= 0),
    child_count      INTEGER NOT NULL CHECK (child_count >= 0),
    evidence_count   INTEGER NOT NULL CHECK (evidence_count >= 0),
    PRIMARY KEY(revision, concept_id)
) WITHOUT ROWID;

CREATE INDEX revision_concepts_by_label
    ON revision_concepts(revision, normalized_label, concept_id);

CREATE TABLE revision_edges (
    revision   INTEGER NOT NULL,
    parent_id  INTEGER NOT NULL,
    child_id   INTEGER NOT NULL,
    CHECK (parent_id <> child_id),
    PRIMARY KEY(revision, parent_id, child_id),
    FOREIGN KEY(revision, parent_id)
        REFERENCES revision_concepts(revision, concept_id)
        ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(revision, child_id)
        REFERENCES revision_concepts(revision, concept_id)
        ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED
) WITHOUT ROWID;

CREATE INDEX revision_edges_by_child
    ON revision_edges(revision, child_id, parent_id);

CREATE TABLE revision_evidence (
    revision    INTEGER NOT NULL,
    concept_id  INTEGER NOT NULL,
    work_id     INTEGER NOT NULL
                    REFERENCES works(id) ON DELETE RESTRICT,
    start_byte  INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte    INTEGER NOT NULL
                    CHECK (end_byte > start_byte AND end_byte - start_byte <= 8192),
    PRIMARY KEY(revision, concept_id, work_id, start_byte, end_byte),
    FOREIGN KEY(revision, concept_id)
        REFERENCES revision_concepts(revision, concept_id)
        ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED
) WITHOUT ROWID;

CREATE INDEX revision_evidence_by_work_range
    ON revision_evidence(revision, work_id, start_byte, end_byte, concept_id);

CREATE TRIGGER revision_snapshots_immutable_update
BEFORE UPDATE ON revision_snapshots BEGIN
    SELECT RAISE(ABORT, 'revision snapshots are immutable');
END;

CREATE TRIGGER revision_snapshots_immutable_delete
BEFORE DELETE ON revision_snapshots BEGIN
    SELECT RAISE(ABORT, 'revision snapshots are immutable');
END;

CREATE TRIGGER revision_concepts_immutable_update
BEFORE UPDATE ON revision_concepts BEGIN
    SELECT RAISE(ABORT, 'revision concepts are immutable');
END;

CREATE TRIGGER revision_concepts_immutable_delete
BEFORE DELETE ON revision_concepts BEGIN
    SELECT RAISE(ABORT, 'revision concepts are immutable');
END;

CREATE TRIGGER revision_edges_immutable_update
BEFORE UPDATE ON revision_edges BEGIN
    SELECT RAISE(ABORT, 'revision edges are immutable');
END;

CREATE TRIGGER revision_edges_immutable_delete
BEFORE DELETE ON revision_edges BEGIN
    SELECT RAISE(ABORT, 'revision edges are immutable');
END;

CREATE TRIGGER revision_evidence_immutable_update
BEFORE UPDATE ON revision_evidence BEGIN
    SELECT RAISE(ABORT, 'revision evidence is immutable');
END;

CREATE TRIGGER revision_evidence_immutable_delete
BEFORE DELETE ON revision_evidence BEGIN
    SELECT RAISE(ABORT, 'revision evidence is immutable');
END;

PRAGMA user_version = 1;

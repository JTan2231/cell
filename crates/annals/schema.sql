-- Annals schema version 3 is intentionally incompatible with every earlier
-- schema.  The corpus has no materialized HEAD and no stored snapshots: typed
-- commit effects are the only authoritative corpus history.

CREATE TABLE library_identity (
    singleton   INTEGER PRIMARY KEY CHECK (singleton = 1),
    library_id  TEXT NOT NULL UNIQUE
                    CHECK (
                        length(library_id) = 32
                        AND library_id = lower(library_id)
                        AND library_id NOT GLOB '*[^0-9a-f]*'
                    )
);

INSERT INTO library_identity(singleton, library_id)
VALUES (1, lower(hex(randomblob(16))));

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

-- IDs may be reserved by pending or superseded requests.  Only a create effect
-- makes an identity part of corpus state, and IDs are never reused.
CREATE TABLE concept_identities (
    id INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0)
);

CREATE TRIGGER concept_identities_immutable_update
BEFORE UPDATE ON concept_identities BEGIN
    SELECT RAISE(ABORT, 'concept identities are immutable');
END;

CREATE TRIGGER concept_identities_immutable_delete
BEFORE DELETE ON concept_identities BEGIN
    SELECT RAISE(ABORT, 'concept identities are immutable');
END;

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

-- arguments/result are immutable audit artifacts.  Annals records and hashes
-- them but never decodes them to drive validation, replay, or application.
CREATE TABLE tool_calls (
    model_run_id    INTEGER NOT NULL REFERENCES model_runs(id) ON DELETE CASCADE,
    sequence        INTEGER NOT NULL CHECK (sequence >= 0),
    tool_name       TEXT NOT NULL,
    arguments       TEXT NOT NULL,
    arguments_sha256 TEXT NOT NULL
                         CHECK (length(arguments_sha256) = 64),
    result          TEXT NOT NULL,
    result_sha256   TEXT NOT NULL CHECK (length(result_sha256) = 64),
    succeeded       INTEGER NOT NULL CHECK (succeeded IN (0, 1)),
    created_at      TEXT NOT NULL,
    PRIMARY KEY(model_run_id, sequence)
) WITHOUT ROWID;

CREATE TRIGGER tool_calls_immutable_update
BEFORE UPDATE ON tool_calls BEGIN
    SELECT RAISE(ABORT, 'tool calls are immutable audit records');
END;

CREATE TRIGGER tool_calls_immutable_delete
BEFORE DELETE ON tool_calls BEGIN
    SELECT RAISE(ABORT, 'tool calls are immutable audit records');
END;

-- A request is shared by a mutable model-run draft and the reconciliation that
-- finalizes it.  Manual submissions create a request directly.  No request JSON
-- or resolved snapshot is authoritative.
CREATE TABLE reconciliation_requests (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    work_id        INTEGER NOT NULL REFERENCES works(id) ON DELETE RESTRICT,
    base_revision  INTEGER NOT NULL CHECK (base_revision >= 0),
    summary        TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    created_at     TEXT NOT NULL
);

CREATE TABLE request_annotations (
    request_id  INTEGER NOT NULL
                    REFERENCES reconciliation_requests(id) ON DELETE CASCADE,
    ordinal     INTEGER NOT NULL CHECK (ordinal >= 0),
    text        TEXT NOT NULL CHECK (length(trim(text)) > 0),
    PRIMARY KEY(request_id, ordinal)
) WITHOUT ROWID;

-- One row owns the variant discriminator and scalar fields.  Selectors and
-- evidence are normalized below.  action may be NULL only for a malformed
-- draft slot awaiting replacement; its raw tool input remains an audit artifact.
CREATE TABLE request_operations (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id           INTEGER NOT NULL
                             REFERENCES reconciliation_requests(id) ON DELETE CASCADE,
    slot                 INTEGER NOT NULL CHECK (slot > 0),
    ordinal              INTEGER NOT NULL CHECK (ordinal >= 0),
    action               TEXT CHECK (action IN (
                             'create_concept', 'add_parent', 'remove_parent',
                             'add_evidence', 'remove_evidence', 'reword_concept',
                             'retire_concept'
                         )),
    local_ref            TEXT,
    label                TEXT,
    evidence_disposition TEXT CHECK (evidence_disposition IN ('retain', 'remove')),
    created_concept_id   INTEGER REFERENCES concept_identities(id) ON DELETE RESTRICT,
    status               TEXT NOT NULL CHECK (status IN (
                             'staged', 'needs_revision', 'blocked', 'implicated', 'dropped'
                         )),
    hint                 TEXT,
    created_version      INTEGER NOT NULL CHECK (created_version >= 1),
    last_changed_version INTEGER NOT NULL CHECK (last_changed_version >= created_version),
    UNIQUE(request_id, slot),
    UNIQUE(request_id, ordinal),
    CHECK (
        action IS NULL
        OR (action = 'create_concept' AND local_ref IS NOT NULL AND label IS NOT NULL
            AND evidence_disposition IS NULL AND created_concept_id IS NOT NULL)
        OR (action = 'reword_concept' AND local_ref IS NULL AND label IS NOT NULL
            AND evidence_disposition IS NOT NULL AND created_concept_id IS NULL)
        OR (action NOT IN ('create_concept', 'reword_concept')
            AND local_ref IS NULL AND label IS NULL
            AND evidence_disposition IS NULL AND created_concept_id IS NULL)
    )
);

CREATE TABLE operation_selectors (
    operation_id  INTEGER NOT NULL
                      REFERENCES request_operations(id) ON DELETE CASCADE,
    role          TEXT NOT NULL CHECK (role IN ('concept', 'parent', 'replacement')),
    ordinal       INTEGER NOT NULL CHECK (ordinal >= 0),
    selector_kind TEXT NOT NULL CHECK (selector_kind IN ('existing', 'local')),
    concept_id    INTEGER REFERENCES concept_identities(id) ON DELETE RESTRICT,
    local_ref     TEXT,
    PRIMARY KEY(operation_id, role, ordinal),
    CHECK (
        (selector_kind = 'existing' AND concept_id IS NOT NULL AND local_ref IS NULL)
        OR (selector_kind = 'local' AND concept_id IS NULL AND local_ref IS NOT NULL)
    )
) WITHOUT ROWID;

CREATE TABLE operation_evidence (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id   INTEGER NOT NULL
                       REFERENCES request_operations(id) ON DELETE CASCADE,
    ordinal        INTEGER NOT NULL CHECK (ordinal >= 0),
    quote          TEXT NOT NULL CHECK (length(trim(quote)) > 0),
    preceded_by    TEXT,
    followed_by    TEXT,
    UNIQUE(operation_id, ordinal)
);

CREATE TABLE operation_evidence_headings (
    evidence_id  INTEGER NOT NULL
                     REFERENCES operation_evidence(id) ON DELETE CASCADE,
    ordinal      INTEGER NOT NULL CHECK (ordinal >= 0),
    component    TEXT NOT NULL CHECK (length(trim(component)) > 0),
    PRIMARY KEY(evidence_id, ordinal)
) WITHOUT ROWID;

CREATE TABLE reconciliation_drafts (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    model_run_id       INTEGER NOT NULL REFERENCES model_runs(id) ON DELETE CASCADE,
    request_id         INTEGER NOT NULL UNIQUE
                           REFERENCES reconciliation_requests(id) ON DELETE CASCADE,
    status             TEXT NOT NULL
                           CHECK (status IN ('open', 'finalized', 'discarded', 'abandoned')),
    version            INTEGER NOT NULL CHECK (version >= 1),
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
        OR (status IN ('finalized', 'discarded')
            AND terminal_sequence IS NOT NULL AND completed_at IS NOT NULL)
        OR (status = 'abandoned' AND terminal_sequence IS NULL AND completed_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX reconciliation_drafts_one_open_per_model_run
    ON reconciliation_drafts(model_run_id) WHERE status = 'open';

CREATE UNIQUE INDEX reconciliation_drafts_one_finalized_per_model_run
    ON reconciliation_drafts(model_run_id) WHERE status = 'finalized';

CREATE TABLE reconciliations (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id        INTEGER NOT NULL UNIQUE
                          REFERENCES reconciliation_requests(id) ON DELETE RESTRICT,
    model_run_id      INTEGER REFERENCES model_runs(id) ON DELETE RESTRICT,
    draft_id          INTEGER REFERENCES reconciliation_drafts(id) ON DELETE RESTRICT,
    status            TEXT NOT NULL
                          CHECK (status IN ('pending', 'applied', 'superseded', 'recorded')),
    actor             TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    applied_revision  INTEGER REFERENCES commits(revision) ON DELETE RESTRICT
                          DEFERRABLE INITIALLY DEFERRED,
    CHECK (
        (status = 'applied' AND applied_revision IS NOT NULL)
        OR (status <> 'applied' AND applied_revision IS NULL)
    )
);

CREATE INDEX reconciliations_by_status ON reconciliations(status, id DESC);

CREATE TRIGGER reconciliations_one_pending_per_work_insert
BEFORE INSERT ON reconciliations
WHEN NEW.status = 'pending' AND EXISTS (
    SELECT 1
    FROM reconciliations AS existing
    JOIN reconciliation_requests AS old_request ON old_request.id = existing.request_id
    JOIN reconciliation_requests AS new_request ON new_request.id = NEW.request_id
    WHERE existing.status = 'pending' AND old_request.work_id = new_request.work_id
)
BEGIN
    SELECT RAISE(ABORT, 'a work already has a pending reconciliation');
END;

CREATE TRIGGER reconciliations_one_pending_per_work_update
BEFORE UPDATE OF status, request_id ON reconciliations
WHEN NEW.status = 'pending' AND EXISTS (
    SELECT 1
    FROM reconciliations AS existing
    JOIN reconciliation_requests AS old_request ON old_request.id = existing.request_id
    JOIN reconciliation_requests AS new_request ON new_request.id = NEW.request_id
    WHERE existing.id <> NEW.id
      AND existing.status = 'pending'
      AND old_request.work_id = new_request.work_id
)
BEGIN
    SELECT RAISE(ABORT, 'a work already has a pending reconciliation');
END;

CREATE UNIQUE INDEX reconciliations_one_per_model_run
    ON reconciliations(model_run_id) WHERE model_run_id IS NOT NULL;

CREATE UNIQUE INDEX reconciliations_one_per_draft
    ON reconciliations(draft_id) WHERE draft_id IS NOT NULL;

-- Open drafts own mutable typed intent.  Once a request is reconciled or its
-- draft reaches any terminal state, every normalized request row is frozen.
CREATE VIEW sealed_reconciliation_requests AS
SELECT request_id FROM reconciliations
UNION
SELECT request_id FROM reconciliation_drafts WHERE status <> 'open';

CREATE TRIGGER reconciliation_requests_sealed_update
BEFORE UPDATE ON reconciliation_requests
WHEN EXISTS (
    SELECT 1 FROM sealed_reconciliation_requests WHERE request_id = OLD.id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER reconciliation_requests_sealed_delete
BEFORE DELETE ON reconciliation_requests
WHEN EXISTS (
    SELECT 1 FROM sealed_reconciliation_requests WHERE request_id = OLD.id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER request_annotations_sealed_insert
BEFORE INSERT ON request_annotations
WHEN EXISTS (
    SELECT 1 FROM sealed_reconciliation_requests WHERE request_id = NEW.request_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER request_annotations_sealed_update
BEFORE UPDATE ON request_annotations
WHEN EXISTS (
    SELECT 1 FROM sealed_reconciliation_requests WHERE request_id = OLD.request_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER request_annotations_sealed_delete
BEFORE DELETE ON request_annotations
WHEN EXISTS (
    SELECT 1 FROM sealed_reconciliation_requests WHERE request_id = OLD.request_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER request_operations_sealed_insert
BEFORE INSERT ON request_operations
WHEN EXISTS (
    SELECT 1 FROM sealed_reconciliation_requests WHERE request_id = NEW.request_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER request_operations_sealed_update
BEFORE UPDATE ON request_operations
WHEN EXISTS (
    SELECT 1 FROM sealed_reconciliation_requests WHERE request_id = OLD.request_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER request_operations_sealed_delete
BEFORE DELETE ON request_operations
WHEN EXISTS (
    SELECT 1 FROM sealed_reconciliation_requests WHERE request_id = OLD.request_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER operation_selectors_sealed_insert
BEFORE INSERT ON operation_selectors
WHEN EXISTS (
    SELECT 1
    FROM request_operations AS operation
    JOIN sealed_reconciliation_requests AS sealed
      ON sealed.request_id = operation.request_id
    WHERE operation.id = NEW.operation_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER operation_selectors_sealed_update
BEFORE UPDATE ON operation_selectors
WHEN EXISTS (
    SELECT 1
    FROM request_operations AS operation
    JOIN sealed_reconciliation_requests AS sealed
      ON sealed.request_id = operation.request_id
    WHERE operation.id = OLD.operation_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER operation_selectors_sealed_delete
BEFORE DELETE ON operation_selectors
WHEN EXISTS (
    SELECT 1
    FROM request_operations AS operation
    JOIN sealed_reconciliation_requests AS sealed
      ON sealed.request_id = operation.request_id
    WHERE operation.id = OLD.operation_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER operation_evidence_sealed_insert
BEFORE INSERT ON operation_evidence
WHEN EXISTS (
    SELECT 1
    FROM request_operations AS operation
    JOIN sealed_reconciliation_requests AS sealed
      ON sealed.request_id = operation.request_id
    WHERE operation.id = NEW.operation_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER operation_evidence_sealed_update
BEFORE UPDATE ON operation_evidence
WHEN EXISTS (
    SELECT 1
    FROM request_operations AS operation
    JOIN sealed_reconciliation_requests AS sealed
      ON sealed.request_id = operation.request_id
    WHERE operation.id = OLD.operation_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER operation_evidence_sealed_delete
BEFORE DELETE ON operation_evidence
WHEN EXISTS (
    SELECT 1
    FROM request_operations AS operation
    JOIN sealed_reconciliation_requests AS sealed
      ON sealed.request_id = operation.request_id
    WHERE operation.id = OLD.operation_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER operation_evidence_headings_sealed_insert
BEFORE INSERT ON operation_evidence_headings
WHEN EXISTS (
    SELECT 1
    FROM operation_evidence AS evidence
    JOIN request_operations AS operation ON operation.id = evidence.operation_id
    JOIN sealed_reconciliation_requests AS sealed
      ON sealed.request_id = operation.request_id
    WHERE evidence.id = NEW.evidence_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER operation_evidence_headings_sealed_update
BEFORE UPDATE ON operation_evidence_headings
WHEN EXISTS (
    SELECT 1
    FROM operation_evidence AS evidence
    JOIN request_operations AS operation ON operation.id = evidence.operation_id
    JOIN sealed_reconciliation_requests AS sealed
      ON sealed.request_id = operation.request_id
    WHERE evidence.id = OLD.evidence_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

CREATE TRIGGER operation_evidence_headings_sealed_delete
BEFORE DELETE ON operation_evidence_headings
WHEN EXISTS (
    SELECT 1
    FROM operation_evidence AS evidence
    JOIN request_operations AS operation ON operation.id = evidence.operation_id
    JOIN sealed_reconciliation_requests AS sealed
      ON sealed.request_id = operation.request_id
    WHERE evidence.id = OLD.evidence_id
)
BEGIN
    SELECT RAISE(ABORT, 'reconciliation request is sealed');
END;

-- A commit is provenance plus one of three typed effect sets.  There is no
-- submitted request, resolved operation list, after-state, or snapshot column.
CREATE TABLE commits (
    revision           INTEGER PRIMARY KEY CHECK (revision > 0),
    kind               TEXT NOT NULL CHECK (kind IN ('change', 'revert', 'shake')),
    reconciliation_id  INTEGER REFERENCES reconciliations(id) ON DELETE RESTRICT,
    reverted_revision  INTEGER REFERENCES commits(revision) ON DELETE RESTRICT,
    actor              TEXT NOT NULL,
    created_at         TEXT NOT NULL,
    CHECK (
        (kind = 'change' AND reconciliation_id IS NOT NULL AND reverted_revision IS NULL)
        OR (kind = 'revert' AND reconciliation_id IS NULL AND reverted_revision IS NOT NULL)
        OR (kind = 'shake' AND reconciliation_id IS NULL AND reverted_revision IS NULL)
    )
);

CREATE UNIQUE INDEX commits_one_per_reconciliation
    ON commits(reconciliation_id) WHERE reconciliation_id IS NOT NULL;

CREATE TABLE concept_effects (
    revision    INTEGER NOT NULL REFERENCES commits(revision) ON DELETE RESTRICT,
    ordinal     INTEGER NOT NULL CHECK (ordinal >= 0),
    concept_id  INTEGER NOT NULL REFERENCES concept_identities(id) ON DELETE RESTRICT,
    effect      TEXT NOT NULL CHECK (effect IN ('create', 'reword', 'retire')),
    label       TEXT,
    PRIMARY KEY(revision, ordinal),
    UNIQUE(revision, concept_id),
    CHECK (
        (effect IN ('create', 'reword') AND label IS NOT NULL AND length(trim(label)) > 0)
        OR (effect = 'retire' AND label IS NULL)
    )
) WITHOUT ROWID;

CREATE TABLE parent_edge_effects (
    revision   INTEGER NOT NULL REFERENCES commits(revision) ON DELETE RESTRICT,
    ordinal    INTEGER NOT NULL CHECK (ordinal >= 0),
    parent_id  INTEGER NOT NULL REFERENCES concept_identities(id) ON DELETE RESTRICT,
    child_id   INTEGER NOT NULL REFERENCES concept_identities(id) ON DELETE RESTRICT,
    effect     TEXT NOT NULL CHECK (effect IN ('add', 'remove')),
    PRIMARY KEY(revision, ordinal),
    UNIQUE(revision, parent_id, child_id),
    CHECK (parent_id <> child_id)
) WITHOUT ROWID;

CREATE TABLE evidence_link_effects (
    revision    INTEGER NOT NULL REFERENCES commits(revision) ON DELETE RESTRICT,
    ordinal     INTEGER NOT NULL CHECK (ordinal >= 0),
    concept_id  INTEGER NOT NULL REFERENCES concept_identities(id) ON DELETE RESTRICT,
    work_id     INTEGER NOT NULL REFERENCES works(id) ON DELETE RESTRICT,
    start_byte  INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte    INTEGER NOT NULL
                    CHECK (end_byte > start_byte AND end_byte - start_byte <= 8192),
    effect      TEXT NOT NULL CHECK (effect IN ('add', 'remove')),
    PRIMARY KEY(revision, ordinal),
    UNIQUE(revision, concept_id, work_id, start_byte, end_byte)
) WITHOUT ROWID;

CREATE TRIGGER commits_immutable_update
BEFORE UPDATE ON commits BEGIN
    SELECT RAISE(ABORT, 'commits are immutable');
END;
CREATE TRIGGER commits_immutable_delete
BEFORE DELETE ON commits BEGIN
    SELECT RAISE(ABORT, 'commits are immutable');
END;
CREATE TRIGGER concept_effects_immutable_update
BEFORE UPDATE ON concept_effects BEGIN
    SELECT RAISE(ABORT, 'concept effects are immutable');
END;
CREATE TRIGGER concept_effects_immutable_delete
BEFORE DELETE ON concept_effects BEGIN
    SELECT RAISE(ABORT, 'concept effects are immutable');
END;
CREATE TRIGGER parent_edge_effects_immutable_update
BEFORE UPDATE ON parent_edge_effects BEGIN
    SELECT RAISE(ABORT, 'parent edge effects are immutable');
END;
CREATE TRIGGER parent_edge_effects_immutable_delete
BEFORE DELETE ON parent_edge_effects BEGIN
    SELECT RAISE(ABORT, 'parent edge effects are immutable');
END;
CREATE TRIGGER evidence_link_effects_immutable_update
BEFORE UPDATE ON evidence_link_effects BEGIN
    SELECT RAISE(ABORT, 'evidence link effects are immutable');
END;
CREATE TRIGGER evidence_link_effects_immutable_delete
BEFORE DELETE ON evidence_link_effects BEGIN
    SELECT RAISE(ABORT, 'evidence link effects are immutable');
END;

-- One row per source delivery.  This operational history is outside corpus
-- state, but applied results may name their commit revision.
CREATE TABLE ingestions (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    delivery_key          TEXT UNIQUE CHECK (
                              delivery_key IS NULL OR length(trim(delivery_key)) > 0
                          ),
    source_name           TEXT NOT NULL CHECK (length(source_name) > 0),
    channel               TEXT NOT NULL CHECK (channel IN ('manual', 'inbox')),
    source_size_bytes     INTEGER CHECK (source_size_bytes >= 0),
    source_created_at     TEXT,
    source_modified_at    TEXT,
    first_seen_at         TEXT NOT NULL CHECK (length(trim(first_seen_at)) > 0),
    ingested_at           TEXT,
    completed_at          TEXT,
    status                TEXT NOT NULL CHECK (status IN ('processing', 'completed', 'failed')),
    work_id               INTEGER REFERENCES works(id) ON DELETE RESTRICT,
    new_work              INTEGER CHECK (new_work IN (0, 1)),
    result                TEXT CHECK (result IN ('retained', 'pending', 'applied', 'recorded')),
    result_revision       INTEGER REFERENCES commits(revision) ON DELETE RESTRICT,
    error_code            TEXT,
    error_message         TEXT,
    CHECK ((channel = 'inbox' AND delivery_key IS NOT NULL)
           OR (channel = 'manual' AND delivery_key IS NULL)),
    CHECK ((work_id IS NULL AND new_work IS NULL AND ingested_at IS NULL)
           OR (work_id IS NOT NULL AND new_work IS NOT NULL AND ingested_at IS NOT NULL)),
    CHECK ((error_code IS NULL AND error_message IS NULL)
           OR (error_code IS NOT NULL AND error_message IS NOT NULL)),
    CHECK (
        (status = 'processing' AND completed_at IS NULL AND result IS NULL)
        OR (status = 'completed' AND completed_at IS NOT NULL AND result IS NOT NULL
            AND work_id IS NOT NULL AND error_code IS NULL AND error_message IS NULL)
        OR (status = 'failed' AND completed_at IS NOT NULL AND result IS NULL
            AND error_code IS NOT NULL AND error_message IS NOT NULL)
    ),
    CHECK ((result = 'applied' AND result_revision IS NOT NULL)
           OR ((result IS NULL OR result <> 'applied') AND result_revision IS NULL))
);

CREATE INDEX ingestions_by_created ON ingestions(source_created_at DESC, id DESC);
CREATE INDEX ingestions_by_modified ON ingestions(source_modified_at DESC, id DESC);
CREATE INDEX ingestions_by_first_seen ON ingestions(first_seen_at DESC, id DESC);
CREATE INDEX ingestions_by_ingested ON ingestions(ingested_at DESC, id DESC);
CREATE INDEX ingestions_by_completed ON ingestions(completed_at DESC, id DESC);
CREATE UNIQUE INDEX ingestions_one_new_work_per_work
    ON ingestions(work_id) WHERE new_work = 1;

-- Compatibility read surface for callers that only need identity and HEAD.
-- The revision is derived from the immutable commit log and is never cached.
CREATE VIEW library_state AS
SELECT identity.singleton,
       COALESCE((SELECT MAX(revision) FROM commits), 0) AS revision,
       identity.library_id
FROM library_identity AS identity;

PRAGMA user_version = 3;

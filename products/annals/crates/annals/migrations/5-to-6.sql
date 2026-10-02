-- Preserve previous examination and request history with unknown (NULL)
-- instruction provenance. Annals initializes the default document in the same
-- migration transaction; it does not reinterpret or rewrite corpus history.
-- Instruction revisions are library settings, separate from retained evidence.
CREATE TABLE library_instruction_revisions (
    revision     INTEGER PRIMARY KEY CHECK (revision > 0),
    content      TEXT NOT NULL CHECK (length(trim(content)) > 0),
    sha256       TEXT NOT NULL CHECK (length(sha256) = 64
                                      AND sha256 = lower(sha256)
                                      AND sha256 NOT GLOB '*[^0-9a-f]*'),
    recorded_at  TEXT NOT NULL CHECK (length(trim(recorded_at)) > 0)
);

CREATE TRIGGER library_instruction_revisions_immutable_update
BEFORE UPDATE ON library_instruction_revisions BEGIN
    SELECT RAISE(ABORT, 'library instruction revisions are immutable');
END;

CREATE TRIGGER library_instruction_revisions_immutable_delete
BEFORE DELETE ON library_instruction_revisions BEGIN
    SELECT RAISE(ABORT, 'library instruction revisions are immutable');
END;

CREATE TABLE library_instruction_selection (
    singleton         INTEGER PRIMARY KEY CHECK (singleton = 1),
    current_revision  INTEGER NOT NULL
                          REFERENCES library_instruction_revisions(revision) ON DELETE RESTRICT
);

CREATE TRIGGER library_instruction_selection_forward_only
BEFORE UPDATE ON library_instruction_selection
WHEN NEW.singleton <> OLD.singleton OR NEW.current_revision <> OLD.current_revision + 1 BEGIN
    SELECT RAISE(ABORT, 'library instruction selection must advance by one revision');
END;

CREATE TRIGGER library_instruction_selection_no_delete
BEFORE DELETE ON library_instruction_selection BEGIN
    SELECT RAISE(ABORT, 'library instruction selection cannot be deleted');
END;

ALTER TABLE model_runs ADD COLUMN instruction_revision INTEGER
    REFERENCES library_instruction_revisions(revision) ON DELETE RESTRICT;
ALTER TABLE model_runs ADD COLUMN instruction_context_sha256 TEXT CHECK (
    (instruction_revision IS NULL AND instruction_context_sha256 IS NULL)
    OR (instruction_revision IS NOT NULL AND instruction_context_sha256 IS NOT NULL
        AND length(instruction_context_sha256) = 64
        AND instruction_context_sha256 = lower(instruction_context_sha256)
        AND instruction_context_sha256 NOT GLOB '*[^0-9a-f]*')
);

DROP INDEX model_runs_one_active_context;
CREATE UNIQUE INDEX model_runs_one_active_context
    ON model_runs(work_id, base_revision, model, reasoning_effort, prompt_version,
                  instruction_revision, instruction_context_sha256)
    WHERE status = 'running';

CREATE TRIGGER model_runs_instruction_context_immutable
BEFORE UPDATE OF instruction_revision, instruction_context_sha256 ON model_runs BEGIN
    SELECT RAISE(ABORT, 'model run instruction context is immutable');
END;

ALTER TABLE reconciliation_requests ADD COLUMN instruction_revision INTEGER
    REFERENCES library_instruction_revisions(revision) ON DELETE RESTRICT;

CREATE TRIGGER reconciliation_requests_context_immutable
BEFORE UPDATE OF work_id, base_revision, instruction_revision ON reconciliation_requests BEGIN
    SELECT RAISE(ABORT, 'reconciliation request context is immutable');
END;

PRAGMA user_version = 6;

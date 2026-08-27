CREATE TABLE inbox_retry_events (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    from_job_id        TEXT NOT NULL CHECK (length(trim(from_job_id)) > 0),
    through_job_id     TEXT NOT NULL CHECK (length(trim(through_job_id)) > 0),
    reason             TEXT CHECK (
                           reason IS NULL
                           OR (length(trim(reason)) > 0 AND length(reason) <= 1000)
                       ),
    state              TEXT NOT NULL
                           CHECK (state IN ('preparing', 'running', 'halted', 'completed')),
    active_slot        INTEGER NOT NULL DEFAULT 1 CHECK (active_slot = 1),
    created_at         TEXT NOT NULL CHECK (length(trim(created_at)) > 0),
    ready_at           TEXT,
    completed_at       TEXT,
    last_halted_at     TEXT,
    last_halt_code     TEXT,
    last_halt_message  TEXT,
    CHECK ((last_halted_at IS NULL AND last_halt_code IS NULL AND last_halt_message IS NULL)
           OR (last_halted_at IS NOT NULL AND last_halt_code IS NOT NULL
               AND last_halt_message IS NOT NULL)),
    CHECK ((state = 'preparing' AND ready_at IS NULL AND completed_at IS NULL
            AND last_halted_at IS NULL)
           OR (state IN ('running', 'halted') AND ready_at IS NOT NULL
               AND completed_at IS NULL)
           OR (state = 'completed' AND ready_at IS NOT NULL AND completed_at IS NOT NULL)),
    CHECK (state <> 'halted' OR last_halted_at IS NOT NULL)
);

CREATE UNIQUE INDEX inbox_retry_events_one_active
    ON inbox_retry_events(active_slot) WHERE state <> 'completed';

CREATE TABLE inbox_retry_items (
    event_id                  INTEGER NOT NULL
                                  REFERENCES inbox_retry_events(id) ON DELETE RESTRICT,
    ordinal                   INTEGER NOT NULL CHECK (ordinal >= 0),
    original_job_id           TEXT NOT NULL
                                  CHECK (length(trim(original_job_id)) > 0),
    original_sequence         INTEGER NOT NULL CHECK (original_sequence > 0),
    original_ingestion_id     INTEGER NOT NULL UNIQUE
                                  REFERENCES ingestions(id) ON DELETE RESTRICT,
    original_completed_at     TEXT NOT NULL
                                  CHECK (length(trim(original_completed_at)) > 0),
    original_error_code       TEXT NOT NULL
                                  CHECK (length(trim(original_error_code)) > 0),
    original_error_message    TEXT NOT NULL
                                  CHECK (length(trim(original_error_message)) > 0),
    original_work_id          INTEGER NOT NULL REFERENCES works(id) ON DELETE RESTRICT,
    child_job_id              TEXT UNIQUE,
    child_sequence            INTEGER CHECK (child_sequence > 0),
    child_ingestion_id        INTEGER UNIQUE REFERENCES ingestions(id) ON DELETE RESTRICT,
    PRIMARY KEY(event_id, ordinal),
    UNIQUE(event_id, original_job_id),
    CHECK ((child_job_id IS NULL AND child_sequence IS NULL AND child_ingestion_id IS NULL)
           OR (child_job_id IS NOT NULL AND length(trim(child_job_id)) > 0
               AND child_sequence IS NOT NULL)),
    CHECK (child_ingestion_id IS NULL OR child_ingestion_id <> original_ingestion_id)
) WITHOUT ROWID;

CREATE TRIGGER inbox_retry_events_identity_immutable
BEFORE UPDATE OF from_job_id, through_job_id, reason, active_slot, created_at
ON inbox_retry_events BEGIN
    SELECT RAISE(ABORT, 'retry event identity is immutable');
END;

CREATE TRIGGER inbox_retry_events_state_transition
BEFORE UPDATE OF state ON inbox_retry_events
WHEN NEW.state <> OLD.state
     AND NOT ((OLD.state = 'preparing' AND NEW.state = 'running')
              OR (OLD.state = 'running' AND NEW.state IN ('halted', 'completed'))
              OR (OLD.state = 'halted' AND NEW.state = 'running')) BEGIN
    SELECT RAISE(ABORT, 'invalid retry event state transition');
END;

CREATE TRIGGER inbox_retry_events_completed_immutable
BEFORE UPDATE ON inbox_retry_events
WHEN OLD.state = 'completed' BEGIN
    SELECT RAISE(ABORT, 'completed retry events are immutable');
END;

CREATE TRIGGER inbox_retry_events_no_delete
BEFORE DELETE ON inbox_retry_events BEGIN
    SELECT RAISE(ABORT, 'retry events are immutable audit records');
END;

CREATE TRIGGER inbox_retry_items_validate_original
BEFORE INSERT ON inbox_retry_items
WHEN NOT EXISTS (
    SELECT 1
    FROM ingestions AS original
    WHERE original.id = NEW.original_ingestion_id
      AND original.channel = 'inbox'
      AND original.status = 'failed'
      AND original.error_code <> 'inbox_job_skipped'
      AND original.completed_at = NEW.original_completed_at
      AND original.error_code = NEW.original_error_code
      AND original.error_message = NEW.original_error_message
      AND original.work_id IS NEW.original_work_id
      AND original.delivery_key GLOB ('inbox:' || NEW.original_job_id || ':*')
) BEGIN
    SELECT RAISE(ABORT, 'retry item original is not the matching failed inbox delivery');
END;

CREATE TRIGGER inbox_retry_items_original_immutable
BEFORE UPDATE OF event_id, ordinal, original_job_id, original_sequence,
                 original_ingestion_id, original_completed_at,
                 original_error_code, original_error_message, original_work_id
ON inbox_retry_items BEGIN
    SELECT RAISE(ABORT, 'retry item membership is immutable');
END;

CREATE TRIGGER inbox_retry_items_child_job_once
BEFORE UPDATE OF child_job_id, child_sequence ON inbox_retry_items
WHEN OLD.child_job_id IS NOT NULL
     AND (NEW.child_job_id IS NOT OLD.child_job_id
          OR NEW.child_sequence IS NOT OLD.child_sequence) BEGIN
    SELECT RAISE(ABORT, 'retry child job is immutable once linked');
END;

CREATE TRIGGER inbox_retry_items_validate_child_delivery
BEFORE UPDATE OF child_ingestion_id ON inbox_retry_items
WHEN NEW.child_ingestion_id IS NOT NULL
     AND NOT EXISTS (
         SELECT 1
         FROM ingestions AS child
         WHERE child.id = NEW.child_ingestion_id
           AND child.channel = 'inbox'
           AND child.delivery_key GLOB ('inbox:' || NEW.child_job_id || ':*')
     ) BEGIN
    SELECT RAISE(ABORT, 'retry child is not the matching inbox delivery');
END;

CREATE TRIGGER inbox_retry_items_child_delivery_once
BEFORE UPDATE OF child_ingestion_id ON inbox_retry_items
WHEN OLD.child_ingestion_id IS NOT NULL
     AND NEW.child_ingestion_id IS NOT OLD.child_ingestion_id BEGIN
    SELECT RAISE(ABORT, 'retry child delivery is immutable once linked');
END;

CREATE TRIGGER inbox_retry_items_no_delete
BEFORE DELETE ON inbox_retry_items BEGIN
    SELECT RAISE(ABORT, 'retry items are immutable audit records');
END;

PRAGMA user_version = 4;

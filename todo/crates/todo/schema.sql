-- Todo schema version 1.
-- Domain state is relational; JSON is reserved for protocol boundaries.

BEGIN;

CREATE TABLE todos (
    id            INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    title         TEXT NOT NULL CHECK (
                      length(trim(title)) > 0
                      AND instr(title, char(10)) = 0
                      AND instr(title, char(13)) = 0
                  ),
    note          TEXT NOT NULL CHECK (length(trim(note)) > 0),
    pointer       TEXT NOT NULL CHECK (length(trim(pointer)) > 0),
    source_path   TEXT NOT NULL CHECK (length(source_path) > 0),
    status        TEXT NOT NULL DEFAULT 'open'
                      CHECK (status IN ('open', 'done')),
    created_at    TEXT NOT NULL DEFAULT (
                      strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                  ),
    completed_at  TEXT,
    CHECK (
        (status = 'open' AND completed_at IS NULL)
        OR (status = 'done' AND completed_at IS NOT NULL)
    )
);

CREATE INDEX todos_status_created
    ON todos(status, created_at DESC, id DESC);

CREATE TRIGGER todos_content_immutable
BEFORE UPDATE OF title, note, pointer, source_path, created_at ON todos BEGIN
    SELECT RAISE(ABORT, 'todo content is immutable');
END;

CREATE TRIGGER todos_cannot_be_deleted
BEFORE DELETE ON todos BEGIN
    SELECT RAISE(ABORT, 'todos cannot be deleted');
END;

CREATE TABLE todo_notes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    todo_id     INTEGER NOT NULL
                    REFERENCES todos(id) ON DELETE RESTRICT,
    text        TEXT NOT NULL CHECK (length(trim(text)) > 0),
    created_at  TEXT NOT NULL DEFAULT (
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                )
);

CREATE INDEX todo_notes_parent_order
    ON todo_notes(todo_id, created_at, id);

CREATE TRIGGER todo_notes_immutable_update
BEFORE UPDATE ON todo_notes BEGIN
    SELECT RAISE(ABORT, 'working notes are append-only');
END;

CREATE TRIGGER todo_notes_immutable_delete
BEFORE DELETE ON todo_notes BEGIN
    SELECT RAISE(ABORT, 'working notes are append-only');
END;

PRAGMA user_version = 1;

COMMIT;

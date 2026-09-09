PRAGMA foreign_keys = ON;

BEGIN IMMEDIATE;

CREATE TABLE clockwork_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    product TEXT NOT NULL CHECK (product = 'clockwork'),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1)
) STRICT;

INSERT INTO clockwork_meta(singleton, product, schema_version)
VALUES (1, 'clockwork', 1);

CREATE TABLE IF NOT EXISTS definitions (
    digest TEXT PRIMARY KEY,
    key TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    registered_at INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS definitions_key_registered
    ON definitions(key, registered_at DESC, digest);

CREATE TRIGGER definitions_no_update
BEFORE UPDATE ON definitions
BEGIN
    SELECT RAISE(ABORT, 'definitions are immutable');
END;

CREATE TRIGGER definitions_no_delete
BEFORE DELETE ON definitions
BEGIN
    SELECT RAISE(ABORT, 'definitions are retained');
END;

CREATE TABLE IF NOT EXISTS bindings (
    key TEXT PRIMARY KEY,
    definition_digest TEXT,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    plist_sha256 TEXT,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (definition_digest) REFERENCES definitions(digest),
    CHECK (plist_sha256 IS NULL OR length(plist_sha256) = 64),
    CHECK (enabled = 0 OR plist_sha256 IS NOT NULL)
) STRICT;

CREATE TABLE IF NOT EXISTS activations (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL,
    definition_digest TEXT NOT NULL,
    trigger TEXT NOT NULL CHECK (trigger IN ('manual', 'launchd')),
    state TEXT NOT NULL CHECK (
        state IN (
            'start_failed',
            'running',
            'exited',
            'signaled',
            'timed_out',
            'skipped_overlap',
            'lost'
        )
    ),
    admitted_at INTEGER NOT NULL,
    started_at INTEGER,
    finished_at INTEGER,
    broker_pid INTEGER,
    child_pid INTEGER,
    exit_code INTEGER,
    signal INTEGER,
    detail TEXT,
    FOREIGN KEY (definition_digest) REFERENCES definitions(digest)
) STRICT;

CREATE INDEX IF NOT EXISTS activations_key_admitted
    ON activations(key, admitted_at DESC, id DESC);

CREATE UNIQUE INDEX IF NOT EXISTS one_running_activation_per_key
    ON activations(key) WHERE state = 'running';

CREATE TRIGGER activations_terminal_immutable
BEFORE UPDATE ON activations
WHEN OLD.state != 'running'
BEGIN
    SELECT RAISE(ABORT, 'terminal activations are immutable');
END;

CREATE TRIGGER activations_no_delete
BEFORE DELETE ON activations
BEGIN
    SELECT RAISE(ABORT, 'activation history is retained');
END;

PRAGMA user_version = 1;

COMMIT;

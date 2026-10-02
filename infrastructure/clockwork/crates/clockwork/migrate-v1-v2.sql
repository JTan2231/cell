BEGIN IMMEDIATE;
DROP TABLE clockwork_meta;
CREATE TABLE clockwork_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    product TEXT NOT NULL CHECK (product = 'clockwork'),
    schema_version INTEGER NOT NULL CHECK (schema_version = 2)
) STRICT;
INSERT INTO clockwork_meta(singleton, product, schema_version)
VALUES (1, 'clockwork', 2);

CREATE TABLE incidents (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL,
    activation_id TEXT,
    definition_digest TEXT,
    code TEXT NOT NULL,
    occurrence TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    resumed_at INTEGER,
    email_cli TEXT NOT NULL,
    notification_status TEXT NOT NULL DEFAULT 'pending' CHECK (notification_status IN ('pending', 'accepted', 'uncertain')),
    first_attempt_at INTEGER,
    last_attempt_at INTEGER,
    notification_attempts INTEGER NOT NULL DEFAULT 0,
    notification_generation INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (activation_id) REFERENCES activations(id),
    FOREIGN KEY (definition_digest) REFERENCES definitions(digest)
) STRICT;
CREATE UNIQUE INDEX one_open_incident_per_key ON incidents(key) WHERE resumed_at IS NULL;
CREATE TABLE abends (
    key TEXT NOT NULL,
    occurrence TEXT NOT NULL,
    code TEXT NOT NULL,
    activation_id TEXT,
    incident_id TEXT,
    recorded_at INTEGER NOT NULL,
    PRIMARY KEY(key, occurrence),
    FOREIGN KEY (activation_id) REFERENCES activations(id),
    FOREIGN KEY (incident_id) REFERENCES incidents(id)
) STRICT;
CREATE TRIGGER abends_no_update BEFORE UPDATE ON abends BEGIN
    SELECT RAISE(ABORT, 'abend evidence is immutable');
END;
CREATE TRIGGER abends_no_delete BEFORE DELETE ON abends BEGIN
    SELECT RAISE(ABORT, 'abend evidence is retained');
END;
CREATE TRIGGER incidents_no_delete BEFORE DELETE ON incidents BEGIN
    SELECT RAISE(ABORT, 'incidents are retained');
END;

PRAGMA user_version = 2;
COMMIT;

CREATE TABLE library_state (
    singleton   INTEGER PRIMARY KEY
                        CHECK (singleton = 1),
    revision    INTEGER NOT NULL
                        CHECK (revision >= 0)
);

INSERT INTO library_state(singleton, revision) VALUES (1, 0);

CREATE TABLE nodes (
    id          INTEGER PRIMARY KEY,
    parent_id   INTEGER
                    REFERENCES nodes(id)
                    ON DELETE CASCADE
                    DEFERRABLE INITIALLY DEFERRED,
    kind        TEXT NOT NULL
                    CHECK (kind IN ('topic', 'source')),
    title       TEXT NOT NULL
                    CHECK (length(trim(title)) > 0),
    body        TEXT NOT NULL DEFAULT '',
    position    INTEGER NOT NULL DEFAULT 0
                    CHECK (position >= 0),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX nodes_by_parent
    ON nodes(parent_id, position, id);

CREATE UNIQUE INDEX nodes_unique_child_position
    ON nodes(parent_id, position)
    WHERE parent_id IS NOT NULL;

CREATE UNIQUE INDEX nodes_unique_root_position
    ON nodes(position)
    WHERE parent_id IS NULL;

CREATE TABLE sources (
    node_id      INTEGER PRIMARY KEY
                     REFERENCES nodes(id) ON DELETE CASCADE,
    locator      TEXT,
    media_type   TEXT,
    checksum     TEXT,
    captured_at  TEXT
);

CREATE TABLE search_units (
    id               INTEGER PRIMARY KEY,
    node_id          INTEGER NOT NULL
                         REFERENCES nodes(id) ON DELETE CASCADE,
    unit_no          INTEGER NOT NULL
                         CHECK (unit_no >= 0),
    unit_kind        TEXT NOT NULL
                         CHECK (unit_kind IN ('node', 'passage')),
    title            TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    breadcrumb       TEXT NOT NULL,
    normalized_path  TEXT NOT NULL,
    text             TEXT NOT NULL,
    start_byte       INTEGER,
    end_byte         INTEGER,
    content_hash     TEXT NOT NULL,
    indexer_version  INTEGER NOT NULL,
    CHECK (
        (unit_kind = 'node' AND start_byte IS NULL AND end_byte IS NULL)
        OR
        (unit_kind = 'passage'
         AND start_byte IS NOT NULL
         AND end_byte IS NOT NULL
         AND start_byte >= 0
         AND end_byte >= start_byte)
    ),
    UNIQUE (node_id, unit_no)
);

CREATE INDEX search_units_by_node
    ON search_units(node_id, unit_no);

CREATE INDEX search_units_by_normalized_title
    ON search_units(normalized_title, node_id);

CREATE INDEX search_units_by_normalized_path
    ON search_units(normalized_path, node_id);

CREATE VIRTUAL TABLE search_fts USING fts5(
    title,
    breadcrumb,
    text,
    content = 'search_units',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3 4'
);

CREATE TRIGGER search_units_after_insert
AFTER INSERT ON search_units
BEGIN
    INSERT INTO search_fts(rowid, title, breadcrumb, text)
    VALUES (new.id, new.title, new.breadcrumb, new.text);
END;

CREATE TRIGGER search_units_after_delete
AFTER DELETE ON search_units
BEGIN
    INSERT INTO search_fts(search_fts, rowid, title, breadcrumb, text)
    VALUES ('delete', old.id, old.title, old.breadcrumb, old.text);
END;

CREATE TRIGGER search_units_after_update
AFTER UPDATE ON search_units
BEGIN
    INSERT INTO search_fts(search_fts, rowid, title, breadcrumb, text)
    VALUES ('delete', old.id, old.title, old.breadcrumb, old.text);
    INSERT INTO search_fts(rowid, title, breadcrumb, text)
    VALUES (new.id, new.title, new.breadcrumb, new.text);
END;

CREATE TABLE index_metadata (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);

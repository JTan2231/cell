CREATE TABLE library_state (
    singleton   INTEGER PRIMARY KEY
                        CHECK (singleton = 1),
    revision    INTEGER NOT NULL
                        CHECK (revision >= 0)
);

INSERT INTO library_state(singleton, revision) VALUES (1, 0);

CREATE TABLE raw_inputs (
    id          INTEGER PRIMARY KEY,
    text        TEXT NOT NULL,
    sha256      TEXT NOT NULL
                    CHECK (
                        length(sha256) = 64
                        AND sha256 = lower(sha256)
                        AND sha256 NOT GLOB '*[^0-9a-f]*'
                    ),
    created_at  TEXT NOT NULL
);

CREATE INDEX raw_inputs_by_sha256
    ON raw_inputs(sha256, id);

CREATE TABLE generation_runs (
    id                     INTEGER PRIMARY KEY,
    input_id               INTEGER NOT NULL
                               REFERENCES raw_inputs(id) ON DELETE CASCADE,
    root_node_id           INTEGER
                               REFERENCES nodes(id) ON DELETE SET NULL
                               DEFERRABLE INITIALLY DEFERRED,
    adapter_name           TEXT NOT NULL
                               CHECK (length(trim(adapter_name)) > 0),
    adapter_version        TEXT NOT NULL
                               CHECK (length(trim(adapter_version)) > 0),
    model                  TEXT NOT NULL
                               CHECK (length(trim(model)) > 0),
    reasoning_effort       TEXT NOT NULL
                               CHECK (length(trim(reasoning_effort)) > 0),
    prompt_version         TEXT NOT NULL
                               CHECK (length(trim(prompt_version)) > 0),
    output_schema_version  INTEGER NOT NULL
                               CHECK (output_schema_version > 0),
    node_budget            INTEGER NOT NULL
                               CHECK (node_budget > 0),
    max_depth              INTEGER NOT NULL
                               CHECK (max_depth >= 0),
    max_children           INTEGER NOT NULL
                               CHECK (max_children > 0),
    accepted_proposal_json TEXT NOT NULL
                               CHECK (json_valid(accepted_proposal_json)),
    created_at             TEXT NOT NULL
);

CREATE INDEX generation_runs_by_input
    ON generation_runs(input_id, id);

CREATE TABLE nodes (
    id                 INTEGER PRIMARY KEY,
    parent_id          INTEGER
                           REFERENCES nodes(id)
                           ON DELETE CASCADE
                           DEFERRABLE INITIALLY DEFERRED,
    generation_run_id  INTEGER
                           REFERENCES generation_runs(id) ON DELETE CASCADE,
    text               TEXT NOT NULL
                           CHECK (length(trim(text)) > 0),
    position           INTEGER NOT NULL DEFAULT 0
                           CHECK (position >= 0),
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    UNIQUE (id, generation_run_id)
);

CREATE INDEX nodes_by_parent
    ON nodes(parent_id, position, id);

CREATE INDEX nodes_by_generation_run
    ON nodes(generation_run_id, id);

CREATE UNIQUE INDEX nodes_unique_generation_root
    ON nodes(generation_run_id)
    WHERE parent_id IS NULL AND generation_run_id IS NOT NULL;

CREATE UNIQUE INDEX nodes_unique_child_position
    ON nodes(parent_id, position)
    WHERE parent_id IS NOT NULL;

CREATE UNIQUE INDEX nodes_unique_root_position
    ON nodes(position)
    WHERE parent_id IS NULL;

CREATE TABLE input_units (
    run_id      INTEGER NOT NULL
                    REFERENCES generation_runs(id) ON DELETE CASCADE,
    unit_id     TEXT NOT NULL
                    CHECK (length(trim(unit_id)) > 0),
    start_byte  INTEGER NOT NULL
                    CHECK (start_byte >= 0),
    end_byte    INTEGER NOT NULL
                    CHECK (end_byte > start_byte),
    PRIMARY KEY (run_id, unit_id),
    UNIQUE (run_id, start_byte, end_byte)
);

CREATE INDEX input_units_by_range
    ON input_units(run_id, start_byte, end_byte);

CREATE TABLE node_support (
    node_id  INTEGER NOT NULL,
    run_id   INTEGER NOT NULL,
    unit_id  TEXT NOT NULL,
    PRIMARY KEY (node_id, run_id, unit_id),
    FOREIGN KEY (node_id, run_id)
        REFERENCES nodes(id, generation_run_id) ON DELETE CASCADE,
    FOREIGN KEY (run_id, unit_id)
        REFERENCES input_units(run_id, unit_id) ON DELETE CASCADE
);

CREATE INDEX node_support_by_unit
    ON node_support(run_id, unit_id, node_id);

CREATE TABLE search_units (
    id               INTEGER PRIMARY KEY,
    node_id          INTEGER NOT NULL UNIQUE
                         REFERENCES nodes(id) ON DELETE CASCADE,
    text             TEXT NOT NULL,
    normalized_text  TEXT NOT NULL,
    breadcrumb       TEXT NOT NULL,
    normalized_path  TEXT NOT NULL,
    content_hash     TEXT NOT NULL,
    indexer_version  INTEGER NOT NULL
                         CHECK (indexer_version > 0)
);

CREATE INDEX search_units_by_normalized_text
    ON search_units(normalized_text, node_id);

CREATE INDEX search_units_by_normalized_path
    ON search_units(normalized_path, node_id);

CREATE VIRTUAL TABLE search_fts USING fts5(
    text,
    breadcrumb,
    content = 'search_units',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3 4'
);

CREATE TRIGGER search_units_after_insert
AFTER INSERT ON search_units
BEGIN
    INSERT INTO search_fts(rowid, text, breadcrumb)
    VALUES (new.id, new.text, new.breadcrumb);
END;

CREATE TRIGGER search_units_after_delete
AFTER DELETE ON search_units
BEGIN
    INSERT INTO search_fts(search_fts, rowid, text, breadcrumb)
    VALUES ('delete', old.id, old.text, old.breadcrumb);
END;

CREATE TRIGGER search_units_after_update
AFTER UPDATE ON search_units
BEGIN
    INSERT INTO search_fts(search_fts, rowid, text, breadcrumb)
    VALUES ('delete', old.id, old.text, old.breadcrumb);
    INSERT INTO search_fts(rowid, text, breadcrumb)
    VALUES (new.id, new.text, new.breadcrumb);
END;

CREATE TABLE index_metadata (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);

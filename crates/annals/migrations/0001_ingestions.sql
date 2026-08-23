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

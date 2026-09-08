CREATE TABLE decision_documents (
    observation_id TEXT PRIMARY KEY REFERENCES observations(id),
    document_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK(status IN ('no_decision', 'pending', 'accepted')),
    markdown TEXT,
    source_sha256 TEXT,
    target_library_id TEXT NOT NULL,
    target_config_path TEXT NOT NULL,
    receipt_json TEXT,
    created_at INTEGER NOT NULL,
    CHECK((status='no_decision' AND markdown IS NULL AND source_sha256 IS NULL AND receipt_json IS NULL)
       OR (status='pending' AND markdown IS NOT NULL AND source_sha256 IS NOT NULL AND receipt_json IS NULL)
       OR (status='accepted' AND markdown IS NULL AND source_sha256 IS NOT NULL AND receipt_json IS NOT NULL))
);
PRAGMA user_version = 5;

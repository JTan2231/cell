CREATE TABLE observer_worker (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    running_since INTEGER,
    idle_since INTEGER,
    error_code TEXT,
    error_since INTEGER
);
CREATE TABLE observation_failures (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    observation_id TEXT NOT NULL REFERENCES observations(id),
    attempt_epoch INTEGER NOT NULL,
    failed_at INTEGER NOT NULL,
    code TEXT NOT NULL,
    detail TEXT NOT NULL
);
PRAGMA user_version = 6;

CREATE TABLE decision_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    envelope_version INTEGER NOT NULL CHECK (envelope_version = 1),
    event_kind TEXT NOT NULL CHECK (event_kind IN ('decision_admitted', 'decision_reviewed')),
    decision_id TEXT NOT NULL REFERENCES candidates(id),
    review_id INTEGER REFERENCES reviews(id),
    occurred_at INTEGER NOT NULL,
    envelope_json TEXT NOT NULL
);
CREATE UNIQUE INDEX decision_events_one_admission
ON decision_events(decision_id) WHERE event_kind='decision_admitted';
CREATE UNIQUE INDEX decision_events_one_review
ON decision_events(review_id) WHERE event_kind='decision_reviewed';

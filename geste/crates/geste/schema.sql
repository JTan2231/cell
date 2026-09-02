PRAGMA foreign_keys = ON;

CREATE TABLE geste_meta (
    marker TEXT PRIMARY KEY CHECK(marker = 'geste'),
    schema_version INTEGER NOT NULL CHECK(schema_version = 1)
);

INSERT INTO geste_meta(marker, schema_version) VALUES('geste', 1);

CREATE TABLE episodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL
);

CREATE TABLE episode_revisions (
    episode_id INTEGER NOT NULL REFERENCES episodes(id),
    revision INTEGER NOT NULL CHECK(revision >= 1),
    submitted_sha256 TEXT NOT NULL CHECK(length(submitted_sha256) = 64),
    recorded_at TEXT NOT NULL,
    title TEXT NOT NULL,
    shape TEXT NOT NULL,
    basis_cutoff_at TEXT NOT NULL,
    recorded_by TEXT NOT NULL,
    situation TEXT NOT NULL,
    response TEXT NOT NULL,
    outcome_status TEXT NOT NULL CHECK(outcome_status IN ('solved', 'partial', 'failed', 'unknown')),
    outcome_summary TEXT NOT NULL,
    applicability TEXT NOT NULL,
    PRIMARY KEY(episode_id, revision)
);

CREATE TABLE revision_seals (
    episode_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    sealed_at TEXT NOT NULL,
    PRIMARY KEY(episode_id, revision),
    FOREIGN KEY(episode_id, revision) REFERENCES episode_revisions(episode_id, revision)
);

CREATE TABLE actions (
    episode_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 1),
    value TEXT NOT NULL,
    PRIMARY KEY(episode_id, revision, ordinal),
    FOREIGN KEY(episode_id, revision) REFERENCES episode_revisions(episode_id, revision)
);

CREATE TABLE lessons (
    episode_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 1),
    value TEXT NOT NULL,
    PRIMARY KEY(episode_id, revision, ordinal),
    FOREIGN KEY(episode_id, revision) REFERENCES episode_revisions(episode_id, revision)
);

CREATE TABLE gaps (
    episode_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 1),
    value TEXT NOT NULL,
    PRIMARY KEY(episode_id, revision, ordinal),
    FOREIGN KEY(episode_id, revision) REFERENCES episode_revisions(episode_id, revision)
);

CREATE TABLE settlements (
    episode_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    settlement_id TEXT NOT NULL,
    statement TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('verified', 'unverified')),
    gap_ordinal INTEGER,
    PRIMARY KEY(episode_id, revision, settlement_id),
    FOREIGN KEY(episode_id, revision) REFERENCES episode_revisions(episode_id, revision),
    FOREIGN KEY(episode_id, revision, gap_ordinal) REFERENCES gaps(episode_id, revision, ordinal)
);

CREATE TABLE tags (
    episode_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 1),
    value TEXT NOT NULL,
    normalized TEXT NOT NULL,
    PRIMARY KEY(episode_id, revision, ordinal),
    UNIQUE(episode_id, revision, normalized),
    FOREIGN KEY(episode_id, revision) REFERENCES episode_revisions(episode_id, revision)
);

CREATE TABLE sources (
    episode_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    source_id TEXT NOT NULL,
    system TEXT NOT NULL,
    kind TEXT NOT NULL,
    reference TEXT NOT NULL,
    source_revision TEXT,
    digest TEXT,
    observed_at TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('authority', 'context', 'evidence', 'effect', 'procedure', 'outcome')),
    label TEXT NOT NULL,
    PRIMARY KEY(episode_id, revision, source_id),
    FOREIGN KEY(episode_id, revision) REFERENCES episode_revisions(episode_id, revision)
);

CREATE TABLE source_supports (
    episode_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    source_id TEXT NOT NULL,
    target TEXT NOT NULL,
    PRIMARY KEY(episode_id, revision, source_id, target),
    FOREIGN KEY(episode_id, revision, source_id) REFERENCES sources(episode_id, revision, source_id)
);

CREATE TABLE related_episodes (
    episode_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 1),
    related_episode_id INTEGER NOT NULL,
    related_revision INTEGER NOT NULL CHECK(related_revision >= 1),
    relation TEXT NOT NULL CHECK(relation IN ('builds_on', 'similar_to', 'contrasts_with', 'supersedes')),
    PRIMARY KEY(episode_id, revision, ordinal),
    UNIQUE(episode_id, revision, related_episode_id, related_revision, relation),
    FOREIGN KEY(episode_id, revision) REFERENCES episode_revisions(episode_id, revision),
    FOREIGN KEY(related_episode_id, related_revision) REFERENCES episode_revisions(episode_id, revision)
);

CREATE INDEX episode_revisions_latest ON episode_revisions(episode_id, revision DESC);

CREATE TRIGGER episodes_no_update
BEFORE UPDATE ON episodes BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER episodes_no_delete
BEFORE DELETE ON episodes BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER episode_revisions_no_update
BEFORE UPDATE ON episode_revisions BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER episode_revisions_no_delete
BEFORE DELETE ON episode_revisions BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER revision_seals_no_update
BEFORE UPDATE ON revision_seals BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER revision_seals_no_delete
BEFORE DELETE ON revision_seals BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER actions_no_update
BEFORE UPDATE ON actions BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER actions_no_delete
BEFORE DELETE ON actions BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER lessons_no_update
BEFORE UPDATE ON lessons BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER lessons_no_delete
BEFORE DELETE ON lessons BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER gaps_no_update
BEFORE UPDATE ON gaps BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER gaps_no_delete
BEFORE DELETE ON gaps BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER settlements_no_update
BEFORE UPDATE ON settlements BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER settlements_no_delete
BEFORE DELETE ON settlements BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER tags_no_update
BEFORE UPDATE ON tags BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER tags_no_delete
BEFORE DELETE ON tags BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER sources_no_update
BEFORE UPDATE ON sources BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER sources_no_delete
BEFORE DELETE ON sources BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER source_supports_no_update
BEFORE UPDATE ON source_supports BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER source_supports_no_delete
BEFORE DELETE ON source_supports BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER related_episodes_no_update
BEFORE UPDATE ON related_episodes BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;
CREATE TRIGGER related_episodes_no_delete
BEFORE DELETE ON related_episodes BEGIN
    SELECT RAISE(ABORT, 'immutable_history');
END;

CREATE TRIGGER actions_no_insert_when_sealed
BEFORE INSERT ON actions
WHEN EXISTS(
    SELECT 1 FROM revision_seals
    WHERE episode_id = NEW.episode_id AND revision = NEW.revision
) BEGIN
    SELECT RAISE(ABORT, 'sealed_revision');
END;
CREATE TRIGGER lessons_no_insert_when_sealed
BEFORE INSERT ON lessons
WHEN EXISTS(
    SELECT 1 FROM revision_seals
    WHERE episode_id = NEW.episode_id AND revision = NEW.revision
) BEGIN
    SELECT RAISE(ABORT, 'sealed_revision');
END;
CREATE TRIGGER gaps_no_insert_when_sealed
BEFORE INSERT ON gaps
WHEN EXISTS(
    SELECT 1 FROM revision_seals
    WHERE episode_id = NEW.episode_id AND revision = NEW.revision
) BEGIN
    SELECT RAISE(ABORT, 'sealed_revision');
END;
CREATE TRIGGER settlements_no_insert_when_sealed
BEFORE INSERT ON settlements
WHEN EXISTS(
    SELECT 1 FROM revision_seals
    WHERE episode_id = NEW.episode_id AND revision = NEW.revision
) BEGIN
    SELECT RAISE(ABORT, 'sealed_revision');
END;
CREATE TRIGGER tags_no_insert_when_sealed
BEFORE INSERT ON tags
WHEN EXISTS(
    SELECT 1 FROM revision_seals
    WHERE episode_id = NEW.episode_id AND revision = NEW.revision
) BEGIN
    SELECT RAISE(ABORT, 'sealed_revision');
END;
CREATE TRIGGER sources_no_insert_when_sealed
BEFORE INSERT ON sources
WHEN EXISTS(
    SELECT 1 FROM revision_seals
    WHERE episode_id = NEW.episode_id AND revision = NEW.revision
) BEGIN
    SELECT RAISE(ABORT, 'sealed_revision');
END;
CREATE TRIGGER source_supports_no_insert_when_sealed
BEFORE INSERT ON source_supports
WHEN EXISTS(
    SELECT 1 FROM revision_seals
    WHERE episode_id = NEW.episode_id AND revision = NEW.revision
) BEGIN
    SELECT RAISE(ABORT, 'sealed_revision');
END;
CREATE TRIGGER related_episodes_no_insert_when_sealed
BEFORE INSERT ON related_episodes
WHEN EXISTS(
    SELECT 1 FROM revision_seals
    WHERE episode_id = NEW.episode_id AND revision = NEW.revision
) BEGIN
    SELECT RAISE(ABORT, 'sealed_revision');
END;

PRAGMA user_version = 1;

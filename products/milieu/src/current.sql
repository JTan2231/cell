PRAGMA foreign_keys = ON;

CREATE TABLE company (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL
);

CREATE TABLE job (
    id TEXT PRIMARY KEY NOT NULL,
    employer_id TEXT NOT NULL REFERENCES company(id),
    title TEXT NOT NULL,
    description TEXT,
    work_mode TEXT,
    status TEXT NOT NULL
);

CREATE TABLE source (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    operator_id TEXT REFERENCES company(id)
);

CREATE TABLE location (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL
);

CREATE TABLE job_location (
    job_id TEXT NOT NULL REFERENCES job(id),
    location_id TEXT NOT NULL REFERENCES location(id),
    PRIMARY KEY (job_id, location_id)
);

CREATE TABLE job_source (
    job_id TEXT NOT NULL REFERENCES job(id),
    source_id TEXT NOT NULL REFERENCES source(id),
    url TEXT NOT NULL,
    PRIMARY KEY (job_id, source_id)
);

# Platter data model

Schema two uses one private SQLite database. The core tables are:

| Table | Owned information |
| --- | --- |
| jobs | Canonical opportunity identity, Cast correlation, employer/title and explicit eligible field |
| runs | One preparation's captured inputs, status, timestamps and compact execution correlation |
| artifacts | Immutable bytes, producing run reference, kind, filename, media type and integrity hash |
| editions | Frozen subject/body, date, occurrence identity, idempotency key, delivery state and acceptance |
| edition_attachments | Edition, position and artifact reference |

Settings contain configuration and the current original-template artifact
reference. Maintenance holds are owner-keyed rows. Live admissions use an
advisory lock on the state directory; no persistent PID or lease is used to infer
process liveness.

A packet is a run with its content. Brief and resume-content artifacts contain
the accepted structured domain outputs; source and PDF artifacts contain the
rendered bytes. Imported originals have a null run reference. The captured
posting/career snapshot lives on its run and references its exact template.
Ashby board responses are separately cached in `ashby-cache/BOARD.json` under
the runtime root. Each file holds `response` and its `retrieved_at` download
time. It is reused for less than 14 days, unless the requested posting is
absent. A valid new download atomically replaces it. These disposable files
are excluded from SQLite backups; captured run inputs remain self-contained.

Run executions contain exact Nucleus requests, input fingerprints, compact
runtime state and attempt correlation; they contain no copied tool-call log or
accepted output payload. Successful tool submissions are idempotent by run and
artifact kind. Conflicting accepted content is refused.

All editions have the same representation. Ordinary selection atomically
freezes a message and sets job eligibility false. Retained-material selection
leaves eligibility unchanged. Neither a receipt nor the existence of an edition
derives eligibility. Acceptance records external submission; unresolved sends
remain held even if eligibility is changed later.

Edition read results project packet IDs and attachment hashes from their
referenced artifacts for CLI compatibility. These are not extra stored
edition-item data. Artifact IDs replace path references. Historical attachment
names and bytes are preserved during migration, even where this requires
separate imported frozen artifacts.

There is no artifact deletion API. Fixed content and frozen messages remain
immutable; any future retention policy must preserve every referenced artifact
and all delivery uncertainty. Explicit exports never become dependencies.

Migration imports schema one transactionally, records a complete schema-two
backup, and removes only hashed legacy files on its durable cleanup manifest.
Nucleus tool history is not copied. Unknown remaining regular runtime files
are retained as imported artifacts. The database backup is self-contained for
Platter history; rendering programs and Nucleus runtime/authentication remain
separate dependencies.

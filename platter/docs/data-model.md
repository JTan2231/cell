# Platter data model

Schema three uses one private SQLite database. The core tables are:

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
New runs also capture `resume_editorial`; its absence selects the legacy
brief/resume workflow. The retained draft request is the common writing setup.
Revision copies it and adds only `proposed_draft` and `editorial_review`.

`resume-draft`, `resume-draft-source` and `resume-draft-pdf` are validated
intermediate artifacts. `resume-review` contains the exact UTF-8 Markdown
review of that draft. Its organization and editorial findings are not parsed.
`resume-content`, `resume-source` and `resume-pdf` remain the final outputs.
Draft and revision use the same submission payload and rendering checks.
Their content and rendered bytes commit together. The draft cannot make a
packet ready. Review must complete before revision starts.

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

A URL-selected run creates no separate workflow record. Its occurrence ID uses
the existing ad hoc edition namespace. It resolves a normal Cast job, prepares
or reuses a normal packet, and freezes one ordinary edition while setting the
selected job ineligible.

Edition read results project packet IDs and attachment hashes from their
referenced artifacts for CLI compatibility. These are not extra stored
edition-item data. Artifact IDs replace path references. Historical attachment
names and bytes are preserved during migration, even where this requires
separate imported frozen artifacts.

There is no artifact deletion API. Fixed content and frozen messages remain
immutable; any future retention policy must preserve every referenced artifact
and all delivery uncertainty. Explicit exports never become dependencies.

Migration imports schema one transactionally, records a complete schema-three
backup, and removes only hashed legacy files on its durable cleanup manifest.
Nucleus tool history is not copied. Unknown remaining regular runtime files
are retained as imported artifacts. The database backup is self-contained for
Platter history; rendering programs and Nucleus runtime/authentication remain
separate dependencies.

Schema-two migration advances the database version without changing retained
inputs, requests or artifact bytes. Older binaries refuse schema three. New
stage toolsets coexist with the retained legacy decoders.

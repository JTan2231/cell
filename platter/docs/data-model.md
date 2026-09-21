# Platter data model

Schema six uses one private SQLite database. The core tables are:

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
New runs capture `generation=weaver_projects_v1`, the Jackson editorial policy
and three exact directions: Cell, Wrought and optional shortening. New captures
append the separate project editorial policy to all three directions. The
resolved directions retain that policy with the run. Historical captures retain
their original directions. Executions retain caller-owned Weaver IDs before
authoring starts. `weaver-cell`, `weaver-wrought` and optional `weaver-cell-short` /
`weaver-wrought-short` artifacts store the
provider document views with exact Markdown. `project-bullets` stores the accepted
plain-text pair. Each project contains one through three bullets.

The `platter/draft/2` job writes only brief and Jackson content. The requester adds
`project_bullets` to accepted resume content; the model cannot submit that field.
The final brief, assembled content, LaTeX and PDF commit together. Project source
history remains in Weaver and Nucleus. Platter retains no Annals snapshots.

A projects-only template import stores a new immutable artifact and updates the
default template setting. Captured runs still reference their exact earlier
artifact. The fixed template includes names, descriptions, technologies, links,
order and two marked project bullet regions. Rendering changes only these regions
and Jackson bullets.

Historical `single_draft_v1` captures keep their former combined writer, project
research and source notes. Captures without `generation` retain their original
staged workflows. All retained requests and outputs keep their meanings.

Regeneration runs also capture `regeneration_id`. This request identity and the
captured Cast job identify one ordinary run for retries. Capture commits them
together under mutation admission. Reuse resumes or returns that run; another
job cannot reuse the ID. Regeneration preserves earlier runs and does not enable
the job for daily selection.

The draft submits brief and resume content together. Mechanical checks precede
one transaction retaining `brief`, `resume-content`, `resume-source` and
`resume-pdf`. A declined draft retains only its brief assessment. Rejected
content is not accepted; correction remains within the same job. New runs
produce no intermediate draft or review artifacts.

Historical `resume-draft`, `resume-draft-source`, `resume-draft-pdf` and
`resume-review` artifacts retain their meanings. The review is exact UTF-8
Markdown; its organization and judgments are not parsed. Historical revision
copies the retained writer request and adds only `proposed_draft` and
`editorial_review`. Review must complete before that revision starts.

Ashby board responses are separately cached in `ashby-cache/BOARD.json` under
the runtime root. Each file holds `response` and its `retrieved_at` download
time. It is reused for less than 14 days, unless the requested posting is
absent. A valid new download atomically replaces it. These disposable files
are excluded from SQLite backups. Captured posting and career inputs remain
self-contained; project research uses the external local resources.

Run executions contain exact Nucleus requests, input fingerprints, compact
runtime state and attempt correlation; they contain no copied tool-call log or
accepted output payload. Successful tool submissions are idempotent by run and
artifact kind. Conflicting accepted content is refused.

All editions have the same representation. Ordinary selection atomically
freezes a message and sets job eligibility false. Retained-material selection
leaves eligibility unchanged. Neither a receipt nor the existence of an edition
derives eligibility. Acceptance records external submission; unresolved sends
remain held even if eligibility is changed later.

A posting retrieval failure sets the job ineligible. An initial failure retains
the job without a run because no full posting was captured. A failed freshness
check retains the prepared run as deferred and preserves its artifacts. Daily
preparation skips excluded jobs and tries other candidates. The eligibility
command can explicitly enable the job again.

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

Migration imports schema one transactionally, records a complete schema-six
backup, and removes only hashed legacy files on its durable cleanup manifest.
Nucleus tool history is not copied. Unknown remaining regular runtime files
are retained as imported artifacts. The database backup is self-contained for
Platter history; rendering programs and Nucleus runtime/authentication remain
separate dependencies.

Schema-two through schema-five migration advances the database version without changing retained
inputs, requests or artifact bytes. Older binaries refuse schema six. New
stage toolsets coexist with the retained legacy decoders.

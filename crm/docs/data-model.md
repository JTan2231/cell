# Data model

CRM schema version 1 stores the entire private library in SQLite. Every intake,
delivery, complete case body, summary, advisory, exact requester request, and
tool result that CRM retains is database `TEXT` with a digest where identity
or replay requires it. CRM creates no product-owned Markdown or intake files.

## Durable records

- `crm_meta` identifies CRM schema one in addition to SQLite
  `PRAGMA user_version`.
- `cases` retain stable identity, title, current revision, lifecycle metadata,
  and no mutable content body.
- `deliveries` retain exact raw UTF-8 tell text, its SHA-256 digest, and optional
  caller-supplied label/source reference. Input paths are not retained.
- `case_revisions` retain a case-local positive revision number, complete
  Markdown, one fixed stage, nullable advisory, summary, content digests,
  optional source update, and timestamp. Database triggers refuse revision
  update or deletion.
- `steward_updates` are both queued work and individual steward attempts. They
  bind one case/delivery and retain `queued|running|applied|failed|lost` state,
  frozen base revision/digest, exact typed Nucleus request/digest, mailbox
  cursor, requester/job identities, admission, applied revision, result-post
  acknowledgment, terminal runtime state/detail, predecessor retry link,
  diagnostics, and lifecycle times.
- `mailbox_receipts` retain job and call identity, exact argument digest,
  exact result JSON and digest, and any committed revision. They make
  byte-identical redelivery replay-safe and conflicting reuse detectable.

Database triggers refuse delivery, case-revision, and receipt update or
deletion. Current case and update views are projections over these durable
rows.

These retained rows do not imply a public export surface. Version 0.1 exposes
case revisions, source-update identity, update/delivery identity, and Nucleus
correlation, but not raw delivery bodies, persisted request JSON, or mailbox
receipt JSON. Direct SQLite reads are outside the supported interface.

## Case revisions

Every case has exactly one revision after creation and at most one current
revision. A revision is a complete snapshot with exactly four structured
content fields:

1. full Markdown;
2. stage: `research`, `warranted`, `contacted`, `connected`, `helped`, or
   `closed`;
3. nullable advisory; and
4. summary.

There is no field-level patch or inherited omission. Revision numbers are
contiguous within a case, old revisions are immutable, and the case's current
pointer advances only in the transaction that inserts the new revision.
The stage is authoritative CRM state only because CRM validates and commits it;
it remains an advisory classification rather than real-world authorization or
independent verification.

A non-null advisory is preserved verbatim as part of the revision. Storage
does not turn it into a policy gate. Every read projection must carry it so the
CLI and downstream JSON consumers can render it conspicuously.

## Intake and queued work

`case new` validates its title, input and stage before a transaction, then
commits the case and revision one atomically. With omitted input the stored
Markdown contains the title followed by the suggested `Current picture`,
`People`, `Chronicle`, and `Open threads` sections. Those headings are not
stored as structured fields, parsed, or required in later revisions. The
initial summary is `Initial case` and advisory is null.

`tell` validates its delivery before a transaction, then commits one immutable
delivery row and one queued update atomically. The delivery remains durable
whether the post-commit worker launch succeeds, the requester is offline, or a
later attempt fails. Neither command stores its input path or creates a content
file.

The worker claims an update and freezes the exact current base revision/digest
before building and persisting its immutable request ahead of ambiguous
Nucleus transport. One `steward_updates` row is one attempt. Explicit retry
reuses the same immutable delivery row/text in a successor update with
`retry_of`, a new requester identity, and a new job identity. At most one update
may source a case revision.

At most one update per case is `running`. Queue drain selects the oldest queued
update whose case has no running update, making per-case steward commits serial
while allowing unrelated cases to retain independent queues.
One database-resident worker lease serializes hidden drainers without creating
a lock sidecar. At its tail, a drain owner uses one immediate transaction to
either claim the next eligible queued update or release the lease. A resume
owner atomically releases and detects eligible queued work, then launches one
replacement drainer when needed. Enqueue and either final handoff therefore
serialize without an empty-check/release gap. A contender need wait at most two
seconds: during a long drain the owner will claim the already-committed work,
while work arriving during a long resume receives the replacement drainer.
One drain attempts each already-unsettled update at most once, continues through
eligible queued work after per-update errors, and only then returns the first
unresolved diagnostic; it does not tight-loop the same recoverable attempt.

## Nucleus correlation and commit

Requester program is `crm`; stable requester identity is
`case-steward:UPDATE_ID`; each update has one unique Nucleus job identity. The
request references
immutable toolset `crm/case-steward/1` and contains the frozen base and delivery
text. Nucleus records execution evidence but does not read or decide CRM rows.

The toolset contains exactly one managed tool, `submit_case_revision`. A valid
call supplies the frozen positive base revision plus complete Markdown of at
most 1,048,576 UTF-8 bytes, stage, nullable nonempty advisory of at most 4,000
bytes, and nonempty summary of at most 1,000 bytes.
CRM commits it only while the selected case's current revision still equals the
attempt base. In one transaction it writes the next immutable revision, stores
the receipt and exact result, advances the case head, marks the update applied,
and records the attempt's domain success.

Nucleus completion without that commit is not domain success. Conversely, the
commit remains successful if the harness later fails while receiving the tool
result or finishing. CRM retains whether the successful result was acknowledged
and the later terminal runtime state/detail separately. An applied update is
runtime-settled only after that terminal observation is durable. A stale base or
malformed call never partially advances the case.

## Idempotency and retry

An ambiguous submission may repeat only the byte-identical typed request under
the same job identity. Tool delivery is keyed by attempt and call identity. A
byte-identical replay returns the already bound result; different arguments
under a used identity fail closed.

`resume` preserves the queued, running, or applied-but-runtime-unsettled update
and cannot create a successor merely because progress is uncertain. `retry`
accepts only `failed` or `lost` updates, then creates a successor update with
new identities and retained `retry_of` while reusing the same delivery. There
is no automatic retry, daemon, or scheduler. `update wait` actively launches
queue drain or same-update recovery once on entry, then only polls; it never
changes retry eligibility.

## Initialization, integrity, and migration

`init` is the only command that creates schema one. Repeating it against an
existing supported CRM schema is idempotent. New Unix database bytes are mode
0600 before SQLite opens them; an existing symbolic link or non-regular target
is rejected before open, and opening a regular database also tightens database
and existing sidecar permissions. The packaged deployer creates its default
state directory mode 0700. Ordinary commands refuse absent, foreign,
incomplete, newer, or older schemas and never migrate implicitly.

Doctor checks `PRAGMA user_version`, the six required schema-one tables,
foreign keys, SQLite integrity, secure database/sidecar permissions, strict
Nucleus health/capabilities, and idempotent registration of the immutable
toolset and its input/result schemas. It does not make Nucleus authoritative
for database health.

Any future schema change requires quiescing hidden workers, retaining a
SQLite-aware database backup including relevant WAL/SHM state, an explicit
migration, an old-state fixture, and database-aware rollback. Program rollback
alone must never reinterpret a newer database.

# CRM records

Schema 2 stores the private CRM library in SQLite. Retained Markdown, intake,
requests, and tool results use `TEXT`. Digests support identity and replay.
Input files are transport; CRM does not retain their paths or create content files.

## Record ownership

| Record | Stored meaning |
| --- | --- |
| `crm_meta` | CRM schema marker, paired with SQLite `user_version` |
| `profile_entries` | Current editable career material |
| `cases` | Stable case identity, title, current revision, and lifecycle metadata |
| `deliveries` | Immutable supplied text, SHA-256, and optional label or source reference |
| `case_revisions` | Immutable complete case snapshots, numbered within each case |
| `steward_updates` | Queued work and one steward attempt per row |
| `mailbox_receipts` | Exact arguments and results associated with a job and tool call |

Triggers reject updates or deletion of deliveries, revisions, and tool receipts.
Case and update views derive from these records. Retention does not imply a
public export interface. Use [the CLI](cli.md) for supported reads.

## Profile entries

One entry is one independently editable piece of career material.

| Field | Meaning and limit |
| --- | --- |
| `id` | Stable opaque identity |
| `title` | Trimmed nonempty text; at most 1,000 UTF-8 bytes; not unique |
| `body_md` | Exact Markdown; at most 1,048,576 UTF-8 bytes; may be empty |
| `updated_at` | Latest CRM write time as an RFC 3339 UTC string |

Update atomically replaces the title and full body. It preserves identity and
sets the write time. There is no revision guard; the last committed replacement
wins. Profiles have no history or case foreign key. They do not invoke a steward
or supply automatic context to cases. Source and disclosure notes remain ordinary
Markdown. List order is descending write time, then ID.

## Case revisions

Creation commits a case and revision one together. Each case has one current
revision. Later revision numbers are contiguous, and the current pointer advances
in the transaction that inserts the revision.

Each revision has four structured content fields:

| Field | Meaning |
| --- | --- |
| Markdown | Complete case snapshot; omissions do not inherit prior content |
| Stage | `research`, `warranted`, `contacted`, `connected`, `helped`, or `closed` |
| Advisory | Nullable retained caution; displayed prominently without blocking operations |
| Summary | Description of the revision |

Revision metadata includes digests, timestamp, and an optional source update.
At most one update can source a revision. A stage classifies the stored case;
it does not authorize contact or prove an external outcome.

With no initial body, creation uses a title and the suggested headings
`Current picture`, `People`, `Chronicle`, and `Open threads`. CRM does not parse
or require those headings. The initial summary is `Initial case`; advisory is null.

## Deliveries and updates

`tell` commits one delivery and one queued update atomically. These records
survive failure to launch the worker or complete the agent run.

An update retains its case and delivery, state, frozen base revision and digest,
exact Nucleus request and digest, mailbox cursor, requester and job IDs, admission,
applied revision, result-post acknowledgment, terminal runtime state and detail,
retry predecessor, diagnostics, and lifecycle times.

The states are `queued`, `running`, `applied`, `failed`, and `lost`. At most one
update per case is running. Drain selects the oldest queued update whose case
has no running update. A database lease serializes hidden drainers; see
[worker coordination](architecture.md#case-update-flow).

Requester program `crm` and requester ID `case-steward:UPDATE_ID` correlate the
attempt with one Nucleus job. The exact request is durable before submission.
It references immutable toolset `crm/case-steward/1` and contains the frozen
base and delivery.

## Revision commit and replay

`submit_case_revision` supplies a positive base revision and these bounded values:

- complete Markdown, at most 1,048,576 UTF-8 bytes;
- one fixed stage;
- a null or nonempty advisory, at most 4,000 bytes;
- a nonempty summary, at most 1,000 bytes.

CRM commits only if the case head still matches the attempt's base. One
transaction writes the revision and receipt, advances the head, marks the update
applied, and records domain success. Invalid content or a stale base changes
no revision.

A receipt binds job and call identity to argument and result digests, exact
result JSON, and any committed revision. Identical redelivery returns that
result. Conflicting reuse fails. Ambiguous admission reuses only the exact
request and job ID.

CRM retains result acknowledgment and later terminal runtime state separately.
An applied update is runtime-settled only after terminal observation is durable.
A runtime failure does not reverse the revision commit.

Resume preserves recoverable work. Retry accepts only failed or lost updates
and creates a successor with a new job identity, the same delivery, and `retry_of`.
Wait can launch recovery once on entry, then polls; it does not change retry
eligibility. See [update operations](cli.md#updates).

## Initialization and migration

`init` creates schema 2 and is idempotent for an existing supported CRM schema.
Ordinary commands refuse absent, foreign, incomplete, older, or newer schemas.
They do not migrate implicitly.

New database files are `0600`; the default state directory is `0700`.
Database open rejects symbolic or non-regular targets and tightens database
and existing sidecar permissions. Doctor checks schema objects, foreign keys,
SQLite integrity, permissions, Nucleus readiness, and immutable registration.

Schema-one migration adds the empty profile table and updates both markers.
It preserves case history, deliveries, receipts, queue state, and leases.
Migration requires settled workers and a private SQLite-aware backup. Queued
work can remain queued. A failure before commit leaves schema one intact;
repeating migration on schema two creates no backup.

See [installation and rollback](system-installation.md) for exact backup-path,
quiescence, and restoration requirements. Program rollback alone cannot make
an older binary compatible with a newer database.

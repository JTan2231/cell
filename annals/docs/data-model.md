# Data model

[`crates/annals/schema.sql`](../crates/annals/schema.sql) is the authoritative
SQLite schema. The current schema is version 5. Schema version 3 remains the
deliberate fresh-state boundary: older libraries are rejected, while `migrate`
upgrades version 3 through the additive version-4 retry provenance and
version-5 library profile and decision-account acceptance tables. Existing
version-3 and version-4 libraries migrate as general libraries.

The library stores eight kinds of facts:

1. immutable library identity and role;
2. immutable works and source-delivery receipts;
3. bounded inbox retry-event membership and parent-child delivery provenance;
4. immutable Krisis producer acceptances and their bounded feed projection;
5. durable concept identities;
6. normalized reconciliation intent and examination audit records;
7. immutable commit provenance; and
8. append-only typed corpus effects.

It does not store a current concept graph, a materialized HEAD, revision
snapshots, or JSON used as operational truth. `CorpusState` is an immutable
in-memory value reduced from revision zero through the typed effects. Every
current and historical corpus read uses that same reducer.

`annals-usage` stores no companion schema. It calculates a disposable report
from Nucleus model output, this library's attribution records, and inbox job
receipts. See [Consumption telemetry](telemetry.md).

## Library identity and revision

`library_identity` contains one random persistent identity. `library_state` is
a view that pairs it with `coalesce(max(commits.revision), 0)`. Revision zero
is therefore the empty corpus, and HEAD is derived rather than updated.
`library_profile` contains the database's single immutable kind, either
`general` or `decisions`. Fresh initialization chooses it explicitly, with
`general` as the CLI default; supported migration of earlier schemas assigns
`general`. The profile is the admission boundary: configuration and spool
files may bind a library but cannot change its role.

Opaque cursors bind to the library identity, revision, and request. A backup
preserves the identity. Applying a nonempty reconciliation, a confirmed
nonempty shake, or a revert appends exactly one contiguous revision. Work
retention, examinations, pending or mechanically equal reconciliations, and
reads do not advance it.

## Decision-account acceptance

`decision_account_acceptances` stores one immutable row for each
`(producer, producer_key)` accepted by a dedicated decisions library. Version
one constrains the producer to `krisis`. The row binds exact source SHA-256,
original job ID and acceptance time to one event ID, account schema version,
statement/context/action/result projection, occurrence time and precision, and
one host/thread/turn/item/span authority anchor. Update and delete triggers make
the row append-only.

Its integer `sequence` orders the accepted-account feed. Watermark and item
cursors encode this position together with the persistent library identity,
but their representation is opaque to consumers. Account Markdown remains in
the producer job envelope and, after dispatch, the immutable work. It is not
returned through the feed. Acceptance itself inserts no `ingestions` row;
dispatch begins the ordinary source-delivery lifecycle later.

The spool file `.decision-feed-library.json` binds a dedicated spool to the
same persistent library ID. An original producer envelope also contains
`producer.json`, which records producer, key, exact digest, job, acceptance
time, and the Annals-derived work label. These files are operational receipts,
not corpus state. General inbox jobs have neither file and retain their prior
behavior.

## Immutable works and deliveries

`works` stores a unique normalized label, original label, complete nonempty
UTF-8 text, SHA-256 digest, and retention time. The digest content-addresses
exact bytes. A second delivery of the same bytes selects the existing work;
the original work label remains canonical. Works cannot be updated or deleted.

`ingestions` stores one lifecycle receipt per started manual or inbox source
delivery. Its state is `processing`, `completed`, or `failed`; successful
results are `retained`, `pending`, `applied`, or `recorded`. Only `applied`
names a corpus revision. Captured size and filesystem times describe the
delivery, while `first_seen_at`, `ingested_at`, and `completed_at` describe the
Annals lifecycle.

Queued jobs live outside SQLite. Each filesystem envelope has unchanged source
bytes, an immutable sequence, and a durable `normal` or `priority` lane in its
job receipt. Dispatch finishes any processing job, then selects priority jobs
before normal jobs and follows sequence order within each lane. It creates or
recovers the database delivery record using the envelope's stable delivery key.
A fresh exact-byte duplicate completes as retained without an examination,
reconciliation, or commit.

## Inbox retry provenance

`inbox_retry_events` stores one operator-created bounded recovery action. It
retains the inclusive `from_job_id` and `through_job_id` failed-job anchors,
optional reason, event state, and lifecycle times. The anchors are resolved
against failed inbox deliveries ordered by `(completed_at, ingestion ID)`. No
row represents an open-ended or retry-all selection, and a partial unique index
permits at most one event whose state is not `completed`.

`inbox_retry_items` stores the event's complete frozen ordered membership. Each
item references one original failed delivery and stores an immutable snapshot
of its job ID, sequence, completion time, error, and retained work. Selection
rejects pre-retention failures because they have no durable material digest.
The item also carries the nullable fresh child job and child delivery created
for the retry. Its ordinal is the contiguous zero-based position in resolved
delivery-failure order, not the original job sequence; priority dispatch may
have made those orders differ. The original delivery and envelope remain
terminal and are never updated to describe the retry. An original delivery is
unique across all retry items, enforcing one direct child; if that child fails,
a later event selects the failed child and forms a linear retry chain.

Event identity and every original-item field are immutable, and event or item
rows cannot be deleted. Child job and delivery links are nullable during
publication but become immutable when assigned. Only event lifecycle and halt
fields advance as execution proceeds.

An event is inserted with all of its selected items before child processing.
The event may remain `preparing` while child envelopes are published to the
filesystem spool. Stable event/item provenance makes publication idempotent
across a crash: recovery links or recognizes one child for the item instead of
expanding membership or creating a second attempt. A child delivery is an
ordinary new inbox delivery with additional retry origin. It may recognize the
original retained work, but its retry intent bypasses the fresh-duplicate
completion path and continues into integration.

The child job receipt carries the same provenance at the spool boundary:
`retry_event_id`, `retry_ordinal`, `retry_of_job_id`, and
`retry_of_ingestion_id` are all present together. `retry_reconciliation_id` is
optional and names only the exact reconciliation owned by the original attempt
that the child may validate and reuse. Ordinary receipts have null retry
provenance. Version-5 receipts are read with those fields null and serialize as
version 6 on a later ordinary rewrite; retry selection does not rewrite the
original terminal receipt.

Item outcomes are not stored counters. They are derived from the linked child
delivery and job state as `not_attempted`, `processing`, `applied`, `recorded`,
`failed`, or `skipped`. Event reports aggregate those derived outcomes while
joining the original failure details. A missing child and a queued zero-attempt
child both derive as `not_attempted`. Event state is `preparing`, `running`,
`halted`, or `completed`; continuing a halted event considers only items whose
derived outcome is `not_attempted`.

## Corpus state

`concept_identities` reserves positive durable IDs. Reservation may happen
while a request is pending or later superseded; only a `create` effect makes
the identity present in `CorpusState`, and an identity is never reused.

A reduced `CorpusState` contains:

- present concept IDs and display labels;
- explicit broader-to-narrower parent edges; and
- evidence links from concepts to exact byte ranges in immutable works.

Labels may repeat. IDs, not labels, carry identity. Edges form an unordered
directed acyclic graph, with no primary parent, sibling order, stored path, or
move operation. Roots, leaves, shared concepts, reachability, and search
normalization are derived.

Evidence belongs to a concept as a whole. A link stores a work and one UTF-8
byte range of at most 8 KiB. Every derived leaf must retain evidence. A public
evidence selector contains an exact quotation and optional heading and adjacent
text filters. It selects every occurrence remaining after those filters,
subject to a bounded fan-out; each selected occurrence becomes a separate
range evidence link before commit. Public input never contains source byte
offsets.

## Typed reconciliation intent

External reconciliation JSON is parsed once at ingress. Annals then stores the
request relationally:

- `reconciliation_requests` owns the work, frozen base revision, summary, and
  creation time;
- `request_annotations` stores inert annotations in order;
- `request_operations` stores one of the seven operation discriminators and
  its scalar fields;
- `operation_selectors` stores existing concept IDs or request-local creation
  references by semantic role;
- `operation_evidence` stores evidence-selector quotations and source context;
  and
- `operation_evidence_headings` stores ordered heading components.

Create operations reserve their durable concept ID in
`created_concept_id`. This makes repeated validation and eventual application
stable without treating a local reference as corpus identity.

A malformed draft slot has a null action and no raw JSON payload. Its original
tool call remains only in the hashed audit transcript. Open drafts may update
their normalized rows. Once a request has a reconciliation, or its draft is in
a terminal state, schema triggers seal every request row and child row.

`reconciliation_drafts` owns a request during model-assisted staging. Slots
keep stable positive IDs, original order, status, bounded hint, and version.
Removing a slot marks it `dropped`; it does not delete history. Finalization
links the same request rows to a reconciliation instead of copying or
serializing them.

`reconciliations` references the normalized request and optionally its model
run and draft. Status is `pending`, `applied`, `superseded`, or `recorded`.
At most one reconciliation per work is pending. A mechanically equal projected
state is `recorded` and has no commit. A pending reconciliation is applied only
while HEAD still equals its base revision.

Pending-reconciliation validation, display, and application reconstruct the
typed request and resolve it again against its original base `CorpusState`. No
stored resolved operation list or projected state is trusted.

## Examination audit

`model_runs` binds one liaison examination to a work, base revision, model,
reasoning effort, and prompt version. Status is `running`, `submitted`,
`no_submission`, or `failed`.

`tool_calls` records tool name, sequence, success, time, and immutable argument
and result artifacts with SHA-256 digests. These text artifacts preserve an
audit trail. Their hashes record content identity, but Annals never decodes the
artifacts to resolve, apply, search, diff, revert, or shake the corpus.

## Canonical commit effects

`commits` stores only revision, kind, provenance, actor, and time. A `change`
references one reconciliation, a `revert` references one earlier commit, and a
`shake` references neither.

The authoritative transition is split across three append-only tables:

- `concept_effects`: `create`, `reword`, or `retire` one concept;
- `parent_edge_effects`: `add` or `remove` one explicit parent edge; and
- `evidence_link_effects`: `add` or `remove` one concept/work/range link.

Each effect has a revision-local ordinal and a uniqueness constraint for its
fact. Commits and effects cannot be updated or deleted. Effect reduction is
strict: creation requires absence, rewording and retirement require presence,
edge and evidence additions require absent links and live endpoints, and
removals require existing links. A reducer failure invalidates the library.

Application first reconstructs the typed request, resolves it at its stored
base, derives the projected state, checks that HEAD still equals the base, and
diffs HEAD against the projection. One immediate transaction then inserts the
commit metadata, appends those canonical effects, marks the reconciliation
applied, and updates the ingestion result when applicable. No second state
representation is written.

A shake computes redundant direct parent edges from replayed HEAD and appends
only their removal effects after confirmation. A revert derives the inverse of
the selected transition against current replayed HEAD and appends it as a new
commit; it never erases history.

## Replay-backed reads

`CorpusState` is the sole corpus model. HEAD, `--at`, diff,
reconciliation resolution, apply, shake, and revert reduce the same ordered
effects. Search and bounded graph queries load the selected replayed state into
connection-local temporary query tables with indexes; those tables are an
ephemeral query implementation, not persisted state or authority.

Commit display reconstructs its semantic narrative from typed request rows,
provenance, effects, and replayed before/after states. A resolved operation in
output is therefore derived information, never a stored JSON payload.

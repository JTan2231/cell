# Data model

[`crates/annals/schema.sql`](../crates/annals/schema.sql) is the authoritative
SQLite schema. Schema version 3 is a deliberate fresh-state boundary: older
libraries are rejected, and `migrate` never translates them.

The library stores five kinds of facts:

1. immutable works and source-delivery receipts;
2. durable concept identities;
3. normalized reconciliation intent and examination audit records;
4. immutable commit provenance; and
5. append-only typed corpus effects.

It does not store a current concept graph, a materialized HEAD, revision
snapshots, or JSON used as operational truth. `CorpusState` is an immutable
in-memory value reduced from revision zero through the typed effects. Every
current and historical corpus read uses that same reducer.

The separate `annals-usage` ledger is not part of this schema. See
[Consumption telemetry](telemetry.md).

## Library identity and revision

`library_identity` contains one random persistent identity. `library_state` is
a view that pairs it with `coalesce(max(commits.revision), 0)`. Revision zero
is therefore the empty corpus, and HEAD is derived rather than updated.

Opaque cursors bind to the library identity, revision, and request. A backup
preserves the identity. Applying a nonempty reconciliation, a confirmed
nonempty shake, or a revert appends exactly one contiguous revision. Work
retention, examinations, pending or mechanically equal reconciliations, and
reads do not advance it.

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

Queued work lives outside SQLite. Each filesystem envelope has an immutable
FIFO sequence and source bytes. Dispatch creates or recovers its ingestion
receipt using the envelope's stable delivery key. A fresh exact-byte duplicate
completes as retained without an examination, reconciliation, or commit.

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

Evidence belongs to a concept as a whole. A link stores a work and UTF-8 byte
range of at most 8 KiB. Every derived leaf must retain evidence. Public input
uses exact quotations and optional context; resolution converts these to byte
ranges before commit.

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
- `operation_evidence` stores quotations and source context; and
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

Validation, display, and application reconstruct the typed request and resolve
it again against its original base `CorpusState`. No stored resolved operation
list or projected state is trusted.

## Examination audit

`model_runs` binds one liaison examination to a work, base revision, model,
reasoning effort, and prompt version. Status is `running`, `submitted`,
`no_submission`, or `failed`.

`tool_calls` records tool name, sequence, success, time, and immutable argument
and result artifacts with SHA-256 digests. These text artifacts preserve an
audit trail. Annals verifies their hashes but never decodes them to validate,
replay, apply, search, diff, revert, or shake the corpus.

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

`CorpusState` is the sole corpus model. HEAD, `--at`, diff, validation,
reconciliation resolution, apply, shake, and revert reduce the same ordered
effects. Search and bounded graph queries load the selected replayed state into
connection-local temporary query tables with indexes; those tables are an
ephemeral query implementation, not persisted state or authority.

Commit display reconstructs its semantic narrative from typed request rows,
provenance, effects, and replayed before/after states. A resolved operation in
output is therefore derived information, never a stored JSON payload.

## Validation

`annals validate` checks SQLite integrity and foreign keys, the exact schema
boundary, work and tool-artifact hashes, identity use, contiguous history,
effect order and uniqueness, and every intermediate `CorpusState` invariant.
It derives each revision's canonical effects from consecutive states and
requires them to match storage exactly.

It also reconstructs every reconciliation and terminal draft from normalized
rows, resolves requests at their original bases, verifies pending/applied/
recorded/superseded semantics, and checks change, shake, and revert provenance.
Forbidden materialized corpus tables and JSON authority columns are validation
failures. Validation is read-only and does not repair state.

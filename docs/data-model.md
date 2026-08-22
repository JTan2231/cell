# Data model

`schema.sql` is the authoritative SQLite schema. The database contains five
kinds of state:

1. immutable source works;
2. source-delivery receipts and captured source metadata;
3. the current materialized concept graph and its evidence;
4. immutable relational graph snapshots for addressable revisions; and
5. model examinations, reconciliations, and append-only commits.

Public commands and liaison tools address works by label, concepts by durable
IDs such as `c42`, evidence by quotation, and history by revision. Exact source
ranges and non-concept row identifiers remain private mechanics.

## Revision state

`library_state` contains one nonnegative `revision` and one random persistent
library identity. Revision zero is the empty corpus. Applying a pending
reconciliation, confirming a nonempty shake, or reverting a commit increments
it. Work retention, reconciliation submission, projected corpus states equal to
their bases, cancelled or empty shakes, validation, and backup leave it
unchanged. Opaque paging cursors bind to the library identity as well as their
revision and request; a backup intentionally preserves that identity.

## Immutable works

`works` stores:

- a unique normalized human label;
- the complete UTF-8 text, containing non-whitespace source content;
- a unique SHA-256 digest; and
- the UTC time it was first retained.

Work labels use Unicode NFKC, lowercase expansion, and collapsed whitespace
for equality. The original label and text are retained unchanged. Exact source
bytes are content-addressed: retaining bytes whose digest already exists
selects the original work and label. A normalized label already used by
different bytes is rejected.

Works are independent of corpus topology. Foreign keys use `ON DELETE
RESTRICT`, and there is no public work-deletion command. Retiring or reverting
a concept therefore cannot delete its source work.

## Source deliveries

`ingestions` contains one durable receipt for each source delivered through a
manual command or the filesystem inbox. A manual `work add` or `integrate`
with a new input creates a receipt; selecting an already retained work with
`integrate --work` does not constitute another delivery. Receipt IDs are
private storage mechanics.

A delivery and a work have different identities. Works are content-addressed,
so several deliveries of identical UTF-8 bytes may refer to the same immutable
`works` row. `new_work` records whether that delivery created the work or
recognized an existing digest. The public receipt renders this as `new` or
`duplicate` retention. Work linkage, retention disposition, and `ingested_at`
are written together in the same immediate transaction. A failure before the
bytes can be retained or recognized leaves all three fields null.

Each receipt captures source metadata independently of the work's contents:

- `source_name` is the delivered filename or manual source name;
- `source_size_bytes` is the observed byte length when available and is filled
  from successfully read input when it was initially unknown;
- `source_created_at` and `source_modified_at` are optional filesystem values;
- `first_seen_at` is when Annals first observed the delivery;
- `ingested_at` is when Annals retained the bytes or recognized their existing
  work; and
- `completed_at` is when the delivery reached a terminal success or failure.

All stored times are UTC RFC 3339 timestamps. Filesystem creation and
modification times are snapshots captured at delivery; Annals does not monitor
the original path afterward. Creation time remains null when the operating
system does not provide it and is never replaced with modification time. These
fields describe source-file metadata, not document authorship or dates parsed
from the source text. For inbox files, `first_seen_at` is recorded in the queue
index on the first settling scan and survives later claim and recovery; size,
creation, and modification metadata come from the scan that claims the settled
file. A manual delivery records first-seen and filesystem metadata when the
command begins handling its source, before reading its bytes.

`channel` is `manual` or `inbox`. Manual receipts have no `delivery_key`.
Inbox receipts use a unique stable delivery key so retry and terminal-envelope
recovery select the original row instead of creating another receipt.
Source-bearing manual commands are serialized by a per-library advisory lock.
On lock acquisition, a processing receipt abandoned by an interrupted manual
command becomes failed with `manual_ingestion_interrupted`. Work retention and
the `retained` result for `work add` are atomic, as are an input integration's
`applied` result and referenced corpus commit.

Receipt lifecycle `status` is `processing`, `completed`, or `failed`:

- `processing` has not reached a terminal outcome and has no completion time or
  result. A retryable inbox failure may retain its latest error while the
  delivery remains processing.
- `completed` has a linked work, completion time, and one result: `retained`
  for `work add`, `pending` for a reconciliation left pending, `applied` for a
  reconciliation applied to the corpus, or `recorded` for a mechanically equal
  reconciliation. Only `applied` has `result_revision`, which references its
  commit.
- `failed` has a completion time, error code, and reporting-safe message, with
  no result or result revision. It may retain work linkage and an ingestion
  time when the failure happened after successful retention.

The receipt itself does not advance the corpus revision. Only an `applied`
result corresponds to a corpus commit. The schema indexes created, modified,
first-seen, ingested, and completed times independently in descending order,
with receipt ID as the deterministic tie-breaker for time-based reports.

## Current corpus

### `concepts`

Each row stores a positive durable identity and its display label. The public
spelling of identity `N` is `cN`. IDs remain the same when a concept is
reworded or its relationships change, and historical snapshots retain the IDs
of retired concepts. Deterministic label normalization is computed for search
and presentation; it is not canonical concept state.

Concept labels are not selectors and need not be unique, even after
normalization. Two concepts named “Locks” are distinct if their IDs differ.

Concept rows do not store a parent, path, root flag, leaf flag, primary
placement, or order.

### `concept_edges`

Each row is one explicit `(parent_id, child_id)` relationship from a broader
concept to a narrower concept. The pair is unique, both endpoints must exist,
and a self-edge is forbidden. Application validation additionally rejects
cycles.

Edges are untyped, unevidenced assertions about whole concept identities.
Reachability defines ancestry. Annals stores the asserted direct edge set and
does not force a transitive reduction during reconciliation: a direct `A -> C`
edge may coexist with `A -> B -> C`. An explicit `annals shake` may later
remove that shortcut while retaining `A` as an ancestor of `C`.

Parents are a set: a child may have several, and no edge is primary. A root is
a concept with in-degree zero. A leaf is a concept with out-degree zero. A
shared concept has more than one parent. These are derived properties, not
stored placement state. Removing a concept or edge may therefore make another
concept a root or leaf without changing that concept's identity.

There is no sibling sequence and no canonical root-to-concept path.
Deterministic query ordering exists only for stable output and cursor paging.

### `evidence`

Each evidence row joins one concept to one immutable work and stores an exact
UTF-8 byte range of at most 8 KiB. The composite key prevents duplicate
concept/work/range links. Validation and SQLite constraints enforce the byte
ceiling, range bounds, and UTF-8 boundaries; every derived leaf must have at
least one evidence link.

Evidence supports the concept identity as a whole. It is not duplicated per
parent, attached to an edge, or scoped to one traversal through the graph.
Non-leaf concepts may also carry evidence.

The public contract accepts an exact quotation and optional source context,
resolves that text uniquely, and stores the range internally. Rewording must
explicitly retain or remove the concept's evidence. Retirement removes the
concept's links but not the retained works.

## Examinations and reconciliations

### `model_runs`

One row binds a liaison invocation to a work and frozen base revision. It also
records model, reasoning effort, prompt version, status, final diagnostic
response or failure, and start/completion times. Its random opaque token scopes
the liaison backend and is never a public concept selector.

A run is `running`, `submitted`, `no_submission`, or `failed`. Model runs are
examination records, not corpus revisions. Annals may reuse the newest
successful run for the exact work, base revision, prompt version, model, and
effort. `--reexamine` bypasses reuse.

### `tool_calls`

Every recognized liaison tool call records its sequence, tool name, strict JSON
arguments and result, success flag, and timestamp. These transcripts preserve
bounded inspection, pagination, and retry history without entering the corpus
commit log.

### `reconciliations`

A reconciliation belongs to a work and base revision and optionally to a model
run. It stores:

- status: `pending`, `applied`, `superseded`, or `recorded`;
- the human summary;
- the exact submitted graph-native request;
- its resolved reconciliation and complete projected corpus state;
- the actor;
- creation time and, when applied, revision.

The submitted request may contain inert free-form annotations. Existing
concepts are selected by `cN`; creations declare request-local `ref` handles,
which are resolved to durable IDs. Handles are request-unique, while labels may
repeat.

At most one reconciliation per work is pending. A result against the same or a
later base revision supersedes the previous pending result for that work; an
older-base result does not displace it. A projected corpus state mechanically
equal to its base has status `recorded` and neither an applied revision nor a
commit.

Submission validates and records a projected corpus state. Application later
re-resolves the request and commits it only if HEAD still equals the base
revision.

## Append-only history

`commits` is a linear log keyed by public revision number. A commit records its
optional source work and reconciliation association; `change`, `shake`, or
`revert` kind; summary, actor, and timestamp; original submitted request;
resolved semantic operations; and complete corpus snapshot state. Its parent
is always the preceding revision. A shake has no source work or reconciliation.

Snapshots contain the concepts, explicit edges, and evidence for a revision.
They are full state rather than an edge-event-only reconstruction, so
historical reads preserve duplicate labels, shared concepts, retired
identities, and old evidence exactly.

Commit effects are derived from the parent and resulting snapshots; they are
not stored as a second event log.

### Relational revision snapshots

`revision_snapshots` records the expected concept, edge, and evidence counts
for each positive revision. `revision_concepts`, `revision_edges`, and
`revision_evidence` store that revision's immutable graph rows. Their keys all
begin with `revision`, so bounded historical queries do not parse the commit's
complete JSON snapshot. Each revision concept also stores immutable parent,
child, and evidence counts, so summaries and frontier accounting do not rescan
incident edge sets. Revision zero is the implicit empty graph and has no rows.

These tables preserve the existing full-snapshot history cost rather than
introducing event replay or validity intervals. They are written after the
canonical graph and commit, but before `library_state.revision` is committed,
inside the same transaction. Validation compares every relational revision
with its committed JSON after-state. Schema triggers reject updates or deletes
to both retained works and relational revision rows.

History renders changes at semantic granularity:

- concept created, retired, or reworded;
- parent edge added or removed; and
- evidence added or removed.

There is no move or reorder event. Changing one parent of a shared concept is
one edge change and does not imply changes to its other parents or descendants.

A revert appends another commit; it never updates or deletes the target. Every
accepted transition remains inspectable by revision through `annals change
show --at REVISION`, including its request, resolved operations, exact effects,
actor, and timestamp.

## Atomic reconciliation commit

Applying a pending reconciliation uses one immediate transaction:

1. require HEAD to equal the reconciliation's base revision;
2. re-resolve the original request and compare it with the stored result;
3. validate the complete projected concepts, edges, and evidence;
4. replace current materialized graph state with that projected corpus state;
5. append the commit, guard-and-advance `library_state.revision`, and store the
   matching immutable relational revision snapshot;
6. mark the reconciliation applied; and
7. commit.

Any failure rolls back every step.

## Atomic shake commit

`annals shake` computes HEAD's transitive reduction. Interactive mode reports
its exact edge removals before asking for confirmation. Application starts one
immediate transaction; requires the persistent library identity, revision, and
materialized graph to match the computed plan; validates that ancestor
reachability is unchanged; materializes the reduced graph; appends a `shake`
commit and full snapshot; advances the revision; and commits. A stale plan
fails atomically; an empty or cancelled plan creates no commit.

## Validation

`annals validate` is read-only. It checks SQLite integrity and foreign keys,
retained-work digests, the singleton HEAD record, contiguous linear history,
parseable full snapshots, equality of materialized HEAD with the latest
historical state, equality of every relational graph projection with its
committed after-state, replayable reconciliation, shake, and revert provenance,
and current corpus invariants.

For every corpus snapshot it also checks concept IDs and labels, edge endpoints,
duplicate and self edges, acyclicity, evidence ranges, and leaf grounding. It
does not impose label uniqueness, choose primary parents, require one root, or
derive a canonical path. It permits transitively implied edges; shaking is an
explicit normalization operation, not an invariant.

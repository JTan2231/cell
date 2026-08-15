# Data model

`schema.sql` is the authoritative SQLite schema. The database contains five
kinds of state:

1. immutable source works;
2. the current materialized concept graph and its evidence;
3. immutable relational graph snapshots for addressable revisions;
4. model examinations, reconciliations, and append-only commits; and
5. a rebuildable concept-search projection.

Public commands and liaison tools address works by label, concepts by durable
IDs such as `c42`, evidence by quotation, and history by revision. Exact source
ranges and non-concept row identifiers remain private mechanics.

## Revision state

`library_state` contains one nonnegative `revision` and one random persistent
library identity. Revision zero is the empty corpus. Applying a pending
reconciliation, confirming a nonempty shake, or reverting a commit increments
it. Work retention, reconciliation submission, mechanically equal projections,
cancelled or empty shakes, validation, backup, and reindexing leave it
unchanged. Opaque paging cursors bind to the library identity as well as their
revision and request; a backup intentionally preserves that identity.

## Immutable works

`works` stores:

- a unique normalized human label;
- the complete UTF-8 text, containing non-whitespace source content;
- a unique SHA-256 digest; and
- its UTC creation time.

Work labels use Unicode NFKC, lowercase expansion, and collapsed whitespace
for equality. The original label and text are retained unchanged. Exact source
bytes are content-addressed: retaining bytes whose digest already exists
selects the original work and label. A normalized label already used by
different bytes is rejected.

Works are independent of corpus topology. Foreign keys use `ON DELETE
RESTRICT`, and there is no public work-deletion command. Retiring or reverting
a concept therefore cannot delete its source work.

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
- its resolved reconciliation and complete projected graph;
- the actor;
- creation time and, when applied, revision.

The submitted request may contain inert free-form annotations. Existing
concepts are selected by `cN`; creations declare request-local `ref` handles,
which are resolved to durable IDs. Handles are request-unique, while labels may
repeat.

At most one reconciliation per work is pending. A result against the same or a
later base revision supersedes the previous pending result for that work; an
older-base result does not displace it. A mechanically equal projection has
status `recorded` and neither an applied revision nor a commit.

Submission validates and records a projection. Application later re-resolves
the request and commits it only if HEAD still equals the base revision.

## Append-only history

`commits` is a linear log keyed by public revision number. A commit records its
parent and base revision; optional source work and reconciliation association;
`change`, `shake`, or `revert` kind; summary, actor, timestamp, and metadata;
original submitted request; resolved semantic operations; and complete corpus
snapshot state. A shake has no source work or reconciliation.

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
and metadata.

## Derived search state

`concept_search` contains one rebuildable row per current concept with exact
and normalized label, its deduplicated ancestor-label context, a deterministic
content hash, and indexer version. A shared concept still has one search row,
not one row for every route from a root. `concept_fts` is an external-content
FTS5 table mirrored by triggers. `index_metadata` records the active indexer
version.

The projection is not authoritative. Applying a corpus change, shake, or
revert rebuilds it inside the canonical transaction. `annals reindex` performs
the same rebuild without changing the revision. Ordinary graph search uses the
selected revision's relational concept and edge rows; it does not treat current
derived rows as historical state.

## Atomic reconciliation commit

Applying a pending reconciliation uses one immediate transaction:

1. require HEAD to equal the reconciliation's base revision;
2. re-resolve the original request and compare it with the stored result;
3. validate the complete projected concepts, edges, and evidence;
4. replace current materialized graph state with that projection;
5. rebuild current derived search state;
6. append the commit, guard-and-advance `library_state.revision`, and store the
   matching immutable relational revision snapshot;
7. mark the reconciliation applied; and
8. commit.

Any failure rolls back every step.

## Atomic shake commit

`annals shake` computes HEAD's transitive reduction. Interactive mode reports
its exact edge removals before asking for confirmation. Application starts one
immediate transaction; requires the persistent library identity, revision, and
materialized graph to match the computed plan; validates that ancestor
reachability is unchanged; materializes the reduced graph; rebuilds search
state; appends a `shake` commit and full snapshot; advances the revision; and
commits. A stale plan fails atomically; an empty or cancelled plan creates no
commit.

## Validation

`annals validate` is read-only. It checks SQLite integrity and foreign keys,
retained-work digests, the singleton HEAD record, contiguous linear history,
parseable full snapshots, equality of materialized HEAD with the latest
historical state, equality of every relational graph projection with its
committed after-state, replayable reconciliation, shake, and revert provenance,
and exact agreement of current search rows.

For every graph snapshot it also checks concept IDs and labels, edge endpoints,
duplicate and self edges, acyclicity, evidence ranges, and leaf grounding. It
does not impose label uniqueness, choose primary parents, require one root, or
derive a canonical path. It permits transitively implied edges; shaking is an
explicit normalization operation, not an invariant.

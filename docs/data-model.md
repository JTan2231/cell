# Data model

`schema.sql` is the authoritative SQLite schema. The database contains four
different kinds of state:

1. immutable source works;
2. the current materialized corpus and its evidence;
3. model examinations, reconciliations, and append-only commits; and
4. a rebuildable concept-search projection.

Storage identifiers and source ranges support mechanics and history. Public
commands and liaison tools address works by label, concepts by path, evidence
by quotation, and history by revision.

## Revision state

`library_state` contains one nonnegative `revision`. Revision zero represents
the empty corpus. Only applying a pending reconciliation or a revert increments
it. Work retention, reconciliation submission, mechanically equal projections,
validation, backup, and reindexing leave it unchanged.

## Immutable works

`works` stores:

- a unique normalized human label;
- the complete UTF-8 text, containing non-whitespace source content;
- a unique SHA-256 digest; and
- its UTC creation time.

Labels use Unicode NFKC, lowercase expansion, and collapsed whitespace for
equality. The original label and text are retained unchanged. Exact source
bytes are content-addressed: retaining bytes whose digest is already present
selects the original work and its label instead of creating another row. A
normalized label already used by different bytes is rejected.

Works are independent of corpus topology. Foreign keys use `ON DELETE
RESTRICT`, and there is no public work-deletion command. Retiring or reverting
a concept therefore cannot delete its source work.

## Current corpus

### `concepts`

Each concept stores a durable internal identity, optional parent, canonical
label, normalized label, sibling ordering value, and created/updated revision.
The adjacency list is an ordered forest.

Unique indexes enforce normalized-unique labels and unique positions among
roots and among each parent's children. Application validation additionally
checks parent existence, acyclicity, revision metadata, complete traversal, and
grounding.

A move or reword preserves internal identity. Retirement removes the current
row, while historical commit snapshots retain its former state. A conceptual
replacement is represented by retirement plus creation, with an optional
replacement path recorded in the resolved operation.

### `evidence`

Each evidence row joins one concept to one immutable work and stores an exact
UTF-8 byte range plus creation time. The database prevents duplicate
concept/work/range links. Application validation checks range bounds and UTF-8
boundaries and requires every leaf concept to have at least one evidence link.

The public contract does not expose ranges. It accepts an exact quotation and
optional natural-language context, resolves that text uniquely, and stores the
range internally. Rewording must explicitly retain or remove all evidence on
the concept. Retirement removes the concept's links but not its works.

## Examinations and reconciliations

### `model_runs`

One row binds a liaison invocation to a work and frozen base revision. It also
records the model, reasoning effort, prompt version, status, final diagnostic
response or failure, and start/completion times. Its random opaque token scopes
the liaison backend (and its private MCP transport when used) and is never a
public selector.

A run is `running`, `submitted`, `no_submission`, or `failed`. Model runs are
examination records, not corpus revisions.

Before creating a run, Annals may reuse the newest successful examination for
the same work, base revision, prompt version, model, and reasoning effort.
Runs without a successful submission are not reusable, and `--reexamine`
explicitly bypasses reuse. It also replaces an incomplete run that never
submitted a reconciliation. A successful atomic submission is the examination
deliverable even if process cleanup remains. Because the base revision is part of this identity, later
corpus context permits a fresh interpretation of the same immutable work.

### `tool_calls`

Every recognized liaison tool call that reaches the scoped backend records its
sequence, tool name, strict JSON arguments, strict JSON result, success flag,
and timestamp. These transcripts preserve inspection and retry history without
entering the corpus commit log.

### `reconciliations`

A reconciliation belongs to a work and base revision and optionally to a model
run. It stores:

- status: `pending`, `applied`, `superseded`, or `recorded`;
- the human summary;
- the exact submitted language-level request;
- the fully resolved operations and resulting snapshot;
- the actor;
- creation time and, when applied, revision.

The submitted request contains one or more operations and may contain
free-form annotations. Annotations are retained with that request but are not
duplicated as operational state and never affect corpus validation,
application, or corpus equality.

At most one reconciliation per work is pending. Recording a result against the
same or a later base revision supersedes the previous pending reconciliation
for that work; an older-base result does not displace it. A reconciliation
whose projection is mechanically equal to its base has status `recorded` and
has neither an applied revision nor a commit.

Reconciliation storage is separate from application: submission validates and
records the projected state, while application later checks HEAD and commits a
materialized transition when one exists.

## Append-only history

`commits` is a linear log keyed by public revision number. Every row records:

- parent and base revision;
- optional source work and reconciliation association;
- `change` or `revert` kind;
- summary, actor, timestamp, and metadata;
- original submitted request and resolved semantic operations; and
- complete before-and-after corpus snapshots.

The schema requires `parent_revision = revision - 1` and `base_revision =
parent_revision`. Full snapshots make historical reads, cross-revision diffs,
validation, and inversion direct. They also preserve retired concepts and old
evidence states.

A revert appends another commit. It never updates or deletes the target commit.
Every applied transition remains inspectable by its public corpus revision
with `annals change show --at REVISION`, including an applied reconciliation or
revert's submitted request, resolved semantic operations, and commit metadata.

## Derived search state

`concept_search` contains one row per current concept with exact and normalized
label and complete path, a deterministic content hash, and indexer version.
`concept_fts` is an external-content FTS5 table mirrored by triggers.
`index_metadata` records the active deterministic indexer version.

This projection is not authoritative. Applying a corpus change or revert
rebuilds it inside the same transaction as canonical state. `annals reindex`
performs the same rebuild without changing the revision.

## Atomic reconciliation commit

Applying a pending reconciliation uses one immediate transaction:

1. require HEAD to equal the reconciliation's base revision;
2. re-resolve the original request and compare it with the stored result;
3. validate the complete projected snapshot;
4. replace the current concepts and evidence with that snapshot;
5. rebuild derived search state;
6. append the commit and mark the reconciliation applied;
7. advance `library_state.revision`; and
8. commit.

Any failure rolls back every step.

## Validation

`annals validate` is read-only. It checks SQLite integrity and foreign keys,
FTS integrity, retained-work digests, the singleton HEAD record, contiguous
linear commit history, parseable and connected commit snapshots, equality of
materialized HEAD with the latest historical after-state, current corpus
invariants, and exact agreement of the derived search projection with current
concepts and paths. It also verifies that recorded reconciliations preserve
their base materialized state and are not linked to commits.

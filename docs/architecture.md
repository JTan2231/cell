# Annals architecture

## Boundary and ownership

Annals is one Rust executable and one SQLite database per library. A work is an
immutable source object. The corpus owns concepts and the explicit
broader-to-narrower edges between them. Evidence is many-to-many: a work may
support many concepts and a concept may be supported by many works. Model runs
own examinations and reconciliations, never concepts.

An applied reconciliation whose projected corpus state differs mechanically
from its base, a confirmed nonempty shake, or a revert advances the corpus.
Retaining a work, reading state, running a model, recording a reconciliation,
canceling or finding no removable edges in a shake, validating, and backing up
do not advance the revision. A projected corpus state mechanically equal to its
base is retained as an interpretive record with status `recorded` and has no
commit or revision of its own.

## Source-delivery boundary

A work is the content-addressed identity of immutable bytes. A source delivery
is one occasion on which material is supplied through a manual command or the
filesystem inbox. Several deliveries may select the same work, so Annals
records each delivery separately instead of attaching arrival metadata to the
deduplicated work.

The delivery receipt captures the source name and byte size, optional
filesystem creation and modification times, and the Annals-controlled
first-seen, ingestion, and completion times. It progresses from `processing`
to `completed` or `failed`. Successful retention records whether the work was
new or already present independently from the processing result (`retained`,
`pending`, `applied`, or `recorded`). A failed delivery retains its stable
error code and reporting-safe message, including failures that occur before a
work exists. Raw runner diagnostics are outside this reporting record.

After successful retention, a fresh inbox delivery follows one of two paths. A
new work enters the liaison flow and is applied immediately when its projected
corpus state differs from its base. Bytes that select an already retained work
complete with `duplicate` retention and result `retained`; the envelope moves
to `duplicates/` without an examination, reconciliation, commit, or revision
change. This routing is prospective: historical delivery records and archived
envelopes keep their recorded results and locations.

Inbox job receipts carry a stable delivery key. Recovery selects the same
database receipt through that key, so moving or retrying a durable envelope
cannot create a second source-delivery record. The queue index records the
first observation timestamp along with FIFO sequence, and the claimed
envelope preserves the same captured source metadata used for its identity
check. A recovered job that already links to a reconciliation finishes that
recorded work rather than being reclassified as a fresh duplicate. Manual
`integrate --work` creates no delivery because it selects bytes already
retained in the library. Both manual integration forms continue through the
explicit integration flow even when the selected bytes were retained earlier.

Source-bearing manual commands share one advisory lock per library. Acquiring
that lock finalizes any processing receipt left by an interrupted prior manual
command as failed. `work add` commits retention and its completed receipt in
one transaction; input-form integration commits an applied result in the same
transaction as its corpus revision.

`annals lately` is a read-only projection of these receipts. Its selected time
basis supplies interval membership and reverse-chronological ordering; source
contents, headings, evidence, and conceptual state are outside this read path.

## Corpus graph

Each concept has a durable public ID such as `c42` and a descriptive label.
Labels may repeat; the ID, not the label, carries identity. Rewording and edge
changes preserve that identity.

Concept edges point from broader scopes to narrower scopes. They form an
unordered directed acyclic graph. A concept may have any number of parents,
and no parent is primary. Roots are concepts with no parents; leaves are
concepts with no children. These classifications are derived from the edge set
and may change when one edge is added or removed.

There are no concept paths, placement slots, sibling positions, or subtree
moves. Presentation code uses deterministic ordering only to make output and
pagination repeatable; that order has no corpus meaning. Each edge is an
explicit semantic assertion. Reconciliation accepts an edge even when longer
paths imply the same ancestry and does not remove it merely for that reason;
an explicit shake can remove such shortcuts mechanically.

Evidence is attached to a concept as a whole, not to a parent edge or one view
of the graph. Every projected leaf must have at least one evidence link.

### Read boundary

Ordinary graph reads do not deserialize a complete corpus snapshot. A
`GraphReader` is a lightweight database facade that selects one immutable
revision. It produces bounded owned views containing only the selected
concepts, ID-only edges, and evidence coordinates required by the caller.
Presentation code receives those views and never queries SQLite or reconstructs
corpus-wide ancestry.

Roots, direct relationships, evidence, search, and local graph expansion apply
their limits in SQLite before constructing response objects. Local expansion
has independent depth, node, and internal edge bounds. Work bodies are outside
the graph projection; evidence quotations are sliced from only the selected
page of immutable works.

Complete in-memory snapshots remain explicit exceptional values for
reconciliation resolution, validation, diff, reversion, and shake planning.
They are not part of the interactive read path.

### Transitive reduction

`annals shake` proposes HEAD's transitive reduction: it removes edge `A -> C`
exactly when another directed path still leads from `A` to `C`. The reduced
DAG is a compact representation of the same ancestor-descendant relation; it
does not preserve every original path or direct-neighbor count.

Interactive human mode reports the exact plan and asks once for confirmation.
Application binds to the persistent library identity and the reported revision
and materialized graph. It removes every reported edge, appends a `shake` commit
and full snapshot, and advances the revision in one immediate transaction. A stale,
cancelled, or empty plan writes nothing. `--yes` supplies noninteractive
confirmation. JSON without `--yes` returns an exit-zero informational preview;
a later invocation with `--yes` computes a fresh plan for its current HEAD.

## Liaison flow

`annals integrate` content-addresses or selects one work and reads HEAD. Before
starting a model, Annals may reuse the newest successful reconciliation for
the exact same work, base revision, prompt version, model, and reasoning
effort. `--reexamine` bypasses that reuse. A later revision or different
liaison configuration always creates a fresh examination.

For a fresh examination, Annals creates a model-run record bound to the work
and base revision and starts an isolated Codex app-server process. The selected
Codex executable defaults to the separate `annals-usage` proxy, which forwards
the protocol to real Codex and records token events against the model-run token.
Observation does not alter the liaison prompt, model, reasoning effort, or tool
set; see [Consumption telemetry](telemetry.md). The high-quality default uses
`gpt-5.6-sol` with max reasoning. The low and medium presets use
`gpt-5.6-luna` and `gpt-5.6-terra`, respectively, both with medium reasoning.
An exact model override changes the model while the selected preset continues
to control reasoning effort.

The process uses an empty temporary directory, a private temporary Codex home,
and no execution environment. Shell, web, planning, user-input, multi-agent,
plugin, skill, and other built-in tool sources are disabled. The prompt is a
short pointer containing the work label, base revision, and operating
instructions, not the complete work. Work text is presented through read
tools as source content, never as operating instructions.

The liaison constructs a provisional, best-current reconciliation. It does not
exclude source-grounded material because it appears familiar, minor,
speculative, redundant, obvious, or unlikely to be useful. It chooses coherent
granularity relative to the work and frozen corpus without claiming a unique
or final semantic decomposition.

At thread start Annals supplies exactly six direct, session-scoped tools:

- `work_overview()` returns byte size and a bounded Markdown-heading outline;
- `work_read(regions[])` performs bounded reads by heading path, unique quote,
  beginning/end anchor, or exact continuation quotation;
- `work_search(queries[])` returns compact paragraph excerpts and heading
  paths;
- `corpus_search(queries[])` searches the frozen graph by label and ancestor
  context, with independent cursors and optional descendant scopes;
- `corpus_inspect(requests[])` batches overview, root, concept, direct
  relationship, evidence, and bounded local-graph reads addressed by public
  concept ID; and
- `submit_reconciliation(reconciliation)` records the session's sole semantic
  write request.

Read and search calls accept batches. Responses are bounded, and the liaison
follows opaque cursors or graph frontiers when it needs more context. One
successful `submit_reconciliation` closes the session's write boundary. Failed
submissions are recoverable tool errors and may be corrected. Tool arguments
and results are retained in the model-run transcript.

App-server sends each dynamic tool call back to the host, which dispatches it
to the in-process liaison backend.

The model's final response is diagnostic only. `integrate` succeeds from the
recorded `submit_reconciliation` side effect. If the process fails after a
valid submission, that reconciliation remains the result; if it exits without
one, Annals returns `model_did_not_submit_reconciliation`.

No SQLite write transaction is held while the model examines the work. For an
ordinary unstructured work, sequential reads can continue from exact returned
text. Highly repetitive text may require another natural anchor; exhaustive
sequential traversal is not guaranteed when no unique continuation exists.

## Language-level reconciliation

The host supplies the immutable evidence work and frozen base revision. The
submitted object contains a summary, one or more semantic operations, and
optional inert annotations. It contains neither the work selector nor base
revision.

An existing concept selector uses its public ID:

```json
{"id":"c42"}
```

A created concept declares a request-unique local handle in `ref`. Any
operation in that request may select it with `new`, including a forward
reference:

```json
{"new":"predicate_locking"}
```

Handles identify creations only within one request. They are not labels and
are replaced by durable `cN` IDs when the request resolves. Duplicate concept
labels are valid.

The graph-native operations are:

- `create_concept`, with `ref`, `label`, an unordered `parents` array, and
  evidence from the scoped work;
- `add_parent`, which idempotently ensures one explicit edge, and
  `remove_parent`, which strictly removes one explicit edge;
- `add_evidence` and `remove_evidence`;
- `reword_concept`, preserving the concept ID and explicitly retaining or
  removing its evidence; and
- `retire_concept`, optionally recording a replacement concept.

There is no move operation. Reclassification is expressed as the exact parent
edges to remove and add; unrelated parents and descendants are unchanged.
Retirement is nonrecursive: other concepts survive, and a child that loses its
last parent becomes a root.

Evidence uses an exact quotation from the scoped work. When text repeats,
`within_heading` (a source-document heading path), `preceded_by`, or
`followed_by` disambiguates it. Concept paths do not exist; source heading
paths remain part of evidence location. Public input never uses byte offsets.

## Resolution and validation

Submission parses the strict JSON contract, resolves all base-revision IDs and
request-local handles, and projects the complete final graph in memory. Annals
validates, among other things:

- every public ID and local handle resolves;
- quotations resolve uniquely in the immutable work;
- every edge has two existing, distinct endpoints and occurs at most once;
- the edge set is acyclic;
- roots and leaves agree with the edge set;
- every leaf has evidence;
- evidence ranges are unique valid UTF-8 ranges no larger than 8 KiB; and
- rewording explicitly retains or removes existing evidence.

Labels are not required to be unique. Annals does not choose a primary parent,
invent a path, reorder concepts, repair a reconciliation, assign confidence,
or judge whether a conceptual claim is true. It validates the deterministic
boundary around that judgment.

A stored reconciliation includes its original request, resolved semantic
operations, and complete projected corpus state. If that state differs
mechanically from the base, the current result is `pending`. A result based on
the same or a later revision supersedes the same work's pending
reconciliation. An older-base result remains an examination record without
displacing a newer pending result.

If the projected corpus state is mechanically equal to the base, Annals stores
the result with status `recorded`. This says only that this interpretation
makes no material corpus change. Annotations are excluded from the comparison.

Resolved operations record what the request addressed; they are not a diff.
They retain idempotent ensure operations even when the selected edge or
evidence already exists; reconciliation status, recorded-change effects, and
revision diffs describe the actual state effect.

## Atomic application

Application requires `HEAD == base_revision`, re-resolves the stored request,
and verifies that it produces the recorded projected corpus state. One
immediate SQLite transaction then materializes concepts, edges, and evidence;
appends the commit with its request, resolved operations, and complete corpus
snapshot; stores the same revision in immutable relational graph rows; marks
the reconciliation applied; advances the revision once; and commits all state
together.

Any error rolls back the entire transition. A stale reconciliation fails with
`stale_change`; Annals does not automatically rebase it. Annotations never
block application.

## History and reversion

HEAD remains materialized for writes. Every committed revision also has
immutable relational concept, edge, and evidence rows, so ordinary current and
historical reads select only the required subset with the same graph API.
Commits additionally retain complete JSON snapshots for validation, semantic
diff, inversion, and provenance replay.

`change show --at REVISION` derives that commit's exact effects by comparing
its parent snapshot with its resulting snapshot.

Diffs describe semantic facts: concept creation, retirement, and rewording;
one parent edge added or removed; and evidence added or removed. They do not
report moves or order changes. A concept with two parents remains one identity
in a diff and in every graph view. A shake appears as one commit whose resolved
transition contains its removed parent edges.

`revert REVISION` applies that revision's inverse to current HEAD and appends a
new `revert` commit. It never removes the original commit. Inversion checks the
affected concepts, edges, and evidence against the target transition; a later
conflicting change fails atomically with `revert_conflict`. An unrelated edge
on the same shared concept is not silently discarded.

Retired concepts disappear from HEAD but remain in historical snapshots.
Works are retained independently and are never deleted by concept retirement
or reversion.

# Annals architecture

## Boundaries

Annals is one Rust executable with an owned catalog and separate SQLite
libraries. Each library authoritatively retains source bytes, a corpus of
concepts, parent edges and evidence, its instruction selection, and history.
The consuming application defines the interpretation through stored library
instructions. Model runs own examination and draft provenance, never corpus
facts. Structural invariants do not depend on a library's subject matter.

Annals stores and derives different kinds of data:

- normalized requests and append-only typed effects are durable;
- `CorpusState` is reduced in memory; and
- JSON exists only at external command/tool boundaries and in immutable hashed
  audit artifacts.

Corpus reads replay history. Annals stores no current-graph snapshot.
See [library migration](cli.md#library-operations) for supported schema versions.

## Library selection and instructions

The private Annals catalog resolves unique names to persistent IDs and managed
database, config, and spool paths. Named commands use the ordinary command
handlers after resolution and identity checks. Catalog records reserve a new
library in `provisioning` before filesystem initialization. Repeating creation
resumes that same identity; only complete matching state becomes `ready`.
Creation has no scheduler or model effects. Existing operator-selected paths
remain supported and are not adopted as names automatically.

Every library stores its exact instruction revisions and one current
selection. `instructions set` appends and selects text in one transaction;
identical current bytes are unchanged. Instructions are trusted library
settings, separate from retained evidence. An instruction change leaves corpus
history intact and starts no reinterpretation.

Shared Annals instructions and tool descriptions specify source preservation,
identity, quotations, DAG constraints, and reconciliation mechanics. The exact
selected library text supplies `developerInstructions` in the Nucleus job.
It defines interpretive choices such as what parent relationships mean. The
initial stored frame uses broader/narrower conceptual scope. A custom frame
changes none of the structural validators or source-admission rules.

## One corpus reducer

Revision zero reduces to an empty `CorpusState`. For each contiguous commit,
the reducer loads its ordered concept, parent-edge, and evidence-link effects
and returns a new state. It enforces transition preconditions while applying
each effect and then enforces whole-state invariants, including acyclicity,
valid evidence ranges, and evidence-grounded leaves.

Every behavior that needs corpus facts goes through this reducer:

- HEAD and historical browsing;
- search and graph expansion;
- reconciliation resolution and validation;
- commit display and diff;
- shake planning;
- revert planning.

Historical and current reads use the same reducer. No separate cache needs
synchronization.

### Query boundary

Bounded graph and search operations benefit from SQL joins and indexes. After
replaying the selected revision, Annals projects that in-memory state into
connection-local temporary concept, edge, and evidence tables. Those tables
exist only for the connection and revision being queried. They are disposable
query acceleration, not library state or authority.

## Source deliveries and inbox recovery

A source delivery records one occasion when material is supplied. Several
deliveries can select the same content-addressed work. Registration and producer
acceptance precede delivery; dispatch starts the inbox delivery record.

The database owns delivery and retry records. The filesystem owns queue envelopes.
These stores cannot commit together. Durable receipts let recovery complete the
exact accepted result without starting a second liaison for an attempted job.
Retry events freeze their membership before publishing fresh child envelopes.
Publication is idempotent across interruption.

The activation lock excludes other workers. A shorter control lock orders
registration, priority changes, dispatch, pause, and interruption. Pause lets
current work finish. Deployment maintenance also blocks spool mutations.
See [inbox operations and recovery](inbox.md) for the state transitions.

A decisions library has an immutable kind, expected persistent ID, and bound
spool. Only the dedicated Krisis producer inlet admits sources. Acceptance binds one
producer key to exact bytes; an identical replay returns the original job.
The accepted-document feed exposes complete unchanged text in a fixed committed
prefix with opaque cursors. Intake requires accessible nonblank UTF-8 text and
normal file, size, integrity, and storage checks; it imposes no decision schema.
These checks isolate it from general libraries. They do not authenticate the
operating user. See [account exchange](../chancery/annals/manuals/decision-account-exchange.md).

## Liaison boundary

The liaison runs as a Nucleus job in an isolated Codex app-server session.
Its pointer prompt contains the work label and frozen base revision. It omits
the complete work and repository instructions. Session-scoped tools provide bounded work reading,
corpus browsing, and reconciliation-draft operations. No shell, web, planning,
user-input, or multi-agent tools are exposed.

Annals captures HEAD and the instruction selection, checks exact-context reuse,
and creates the model-run record in one immediate transaction. The record
freezes the instruction revision and a hash of the exact effective prompt and
tool definitions. Reuse and active-run uniqueness also require the same work,
base, model, and reasoning effort. A → B → A instruction changes therefore do
not revive old results. A session loads instructions by its recorded revision,
not by the current selection.

Annals registers immutable liaison toolset version 2 and input schemas ending
in `.input.v2`, submits a deterministically identified Nucleus job, and services
its durable requester mailbox. The tool-result schema remains
`annals.liaison-tool-result.v1`; historical result decoding is preserved. A repeated ambiguous submission carries
byte-identical request content. Tool results are cached before transmission,
so retry after an ambiguous transport failure never executes an Annals backend
operation twice. Annals continues to determine success from the durable
recorded reconciliation, not from the model's final message.

Nucleus exclusively owns Codex process isolation, persistent authentication,
canonical credential refresh, and bounded scheduling. Up to eight jobs may run
concurrently; account reads may overlap them, while attended login waits for
active job and account sessions to settle.
Annals neither reads nor sets `CODEX_HOME` and has no direct-runner fallback.
`[liaison].nucleus_socket` optionally selects a nonstandard Unix socket; its
default is Nucleus's current-user socket. Before the first new queued attempt,
Annals asks Nucleus for an authenticated account preflight and may wait up to
30 seconds for Nucleus's canonical account operation. Failure leaves the
envelope queued with attempts zero and no source-delivery record.

Nucleus retains exact raw app-server output plus authoritative job, attempt,
and pending-tool state. Annals services pending tool calls and watches durable
job state. After completion it derives the final liaison message from ordered
model output when Nucleus has not already projected that terminal value.
Execution diagnostics are not a second durable reporting stream.

Bounded work reads use natural heading, quotation, continuation, or document
edge anchors rather than offsets. A heading or quotation anchor must resolve
uniquely; evidence-selector fan-out does not apply to source reading.

Every tool request crosses a strict JSON ingress boundary. Annals parses it to
language-level types, applies size and shape limits, and stores recognized
reconciliation intent in normalized rows. The raw tool arguments and result
are retained and hashed for audit, but no behavior decodes those artifacts
later.

### Draft staging

The first submission creates a request and an open draft. Each operation has a
stable slot and typed child rows for selectors and evidence. A malformed slot
is represented by a null action plus a repair hint; raw malformed JSON does not
become draft state.

Revision calls replace named slots, mark removals dropped, append new slots,
or change request metadata. Unmentioned slots remain unchanged. Annals assesses
operations individually and then resolves the active set together. When the
whole request succeeds, the draft becomes finalized and its existing request
rows are linked directly to one reconciliation. Discarded and abandoned drafts
remain audit records.

## Resolution

A reconciliation operation is one of:

- create concept;
- add or remove parent edges;
- add or remove evidence;
- reword a concept; or
- retire a concept, optionally with a replacement.

Existing concepts are selected by durable `cN` IDs. Create operations declare
request-local references whose durable IDs are reserved at ingress. Evidence
selectors use exact quotations plus optional heading and adjacent-text context;
each selector selects every occurrence remaining after those filters, subject
to a bounded fan-out. At least one occurrence must remain, and each becomes a
separate exact-range evidence link. Public input never uses source byte
offsets.

Resolution is a pure state transition over the original base `CorpusState`.
It validates local-reference scope, selector cardinality, operation ordering,
reword evidence disposition, retirement replacement semantics, graph
acyclicity, and leaf evidence. It yields a projected state but does not write
one.

Submission stores only normalized intent and reconciliation provenance.
Pending validation, display, and application reload that intent and resolve it
again at the recorded base. A mechanically equal projection is recorded as an
interpretive result without a commit.

## Atomic application

Applying a pending reconciliation opens an immediate transaction and:

1. replays the original base and current HEAD;
2. requires HEAD and the selected instructions to equal their stored revisions;
3. reconstructs and resolves the normalized request;
4. derives the canonical effect set by diffing HEAD and the projection;
5. inserts one commit and its ordered typed effects;
6. marks the reconciliation applied; and
7. completes a linked ingestion result when applicable.

The transaction commits once. A failure changes neither corpus history nor
workflow state. There is no snapshot/materialization step and no serialized
resolved object to keep synchronized.

## History, diff, shake, and revert

Applied changes, confirmed nonempty shakes, and reverts form a contiguous
append-only revision sequence. Work retention, examinations, pending or
recorded reconciliations, previews, and failed attempts do not.

`diff` replays both requested revisions and compares their `CorpusState`
values. `change show --at` combines the stored effects with typed intent and
replayed context to derive its public narrative.

`shake` computes a transitive reduction plan from replayed HEAD. Confirmation
replays HEAD again, rejects a stale plan, and appends only parent-edge removal
effects. It preserves every ancestor-descendant relation while removing
transitively implied direct assertions. Library instructions can give a direct
edge meaning beyond reachability, so shake confirmation binds to the exact
instruction revision and warns about that limit.

`revert` loads the target transition, derives its inverse, and applies that
inverse to current HEAD. If a targeted fact has changed incompatibly since the
original commit, the revert fails atomically. Successful reversion is a new
commit and never removes the original.

## Installation state

Deployment journals library identities, backups, migrations, program selection,
and scheduler state. Recovery restores compatible data before public commands.
See [installation and recovery](system-installation.md) and
[older-installation migration](migration.md).

## Rust document interface

`annals-api` owns exchange contract 2: the acceptance receipt, success envelope,
watermark, fixed page, `AcceptedDocumentEvent`, and typed CLI client. Annals emits
these same types. Each event includes complete accepted text, source filename,
content digest, acceptance time, and transport identities. Krisis, Semantics,
and Conatus use these types and retain their own delivery and processing policy.
No account parser or mandatory source lookup governs document acceptance.

## Resource limits and cost

The selected `CorpusState` is held in memory. Reading revision N replays effects
from revisions 1 through N. Replay and invariant checks grow with the number
of concepts, edges, and evidence links. Annals has no snapshot cache.

Mutation stores canonical differences. Graph queries load a temporary projection
of the whole selected state, then apply page limits before constructing output.
A bounded response therefore does not imply bounded replay cost. Evidence reads
load only selected byte ranges, with an 8 KiB cap per range.

Exact-context examination reuse can avoid a new model run. `--reexamine` bypasses
reuse. These mechanisms specify no latency or throughput guarantee.

| Operation or retained value | Limit | Default |
| --- | --- | --- |
| Liaison execution | 60 minutes | — |
| App-server transcript | 64 MiB | — |
| Retained model-error tail | 64 KiB | — |
| `work_read` regions per call | 1–20 | — |
| Characters per work region | 12,000 | 4,000 |
| Work overview heading characters | 16,000; truncation reported | — |
| Queries per work or corpus search | 1–20 nonempty queries | — |
| Matches per work-search query | 1–10 | 5 |
| Work-search excerpt characters | 1,000 | — |
| Matches per corpus-search query | 1–50, separate cursor per query | 10 |
| Requests per `corpus_inspect` call | 1–20 | — |
| Parent, child, evidence, or root page | 1–100 items | 25 |
| Relation preview in concept inspection | At most 20 items | 5 |
| Local graph | Depth 0–5; at most 500 concepts; frontier reported | — |
| Model-facing evidence excerpt | 2,000 characters; truncation reported | — |

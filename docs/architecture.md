# Annals architecture

## Boundaries

Annals is one Rust executable and one SQLite library. A work owns immutable
source bytes. The corpus owns durable concepts, explicit broader-to-narrower
edges, and concept evidence. Model runs own examinations and draft provenance,
never corpus facts.

The SQLite boundary is intentionally asymmetric:

- normalized requests and append-only typed effects are durable;
- `CorpusState` is reduced in memory; and
- JSON exists only at external command/tool boundaries and in immutable hashed
  audit artifacts.

There is no materialized current graph, stored HEAD snapshot, or alternative
replay path. Schema version 3 rejects every earlier library rather than
attempting to translate competing historical representations. Schema version
4 is an additive migration from version 3 that introduces bounded inbox retry
provenance without changing existing semantic or delivery history.

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
- revert planning; and
- full-library validation.

This keeps historical and current behavior identical by construction. There
is no cache whose agreement must be trusted.

### Query boundary

Bounded graph and search operations benefit from SQL joins and indexes. After
replaying the selected revision, Annals projects that in-memory state into
connection-local temporary concept, edge, and evidence tables. Those tables
exist only for the connection and revision being queried. They are disposable
query acceleration, not library state, and validation never treats them as
authority.

## Source-delivery boundary

A source delivery is distinct from its content-addressed work. Manual commands
and dispatched inbox jobs create ingestion receipts with captured source
metadata and lifecycle status. Several deliveries can select the same work.

The filesystem inbox separates admission from dispatch. Registration moves a
settled source into `queued/JOB_ID/material`, assigns an immutable monotonic
sequence, and writes an unstarted normal-lane job receipt. Direct enqueue
copies explicitly selected files into complete unstarted envelopes, leaves the
originals unchanged, and can select the priority lane without passing through
settling admission. Dispatch finishes any active job, then moves the
lowest-sequence priority envelope, or the lowest-sequence normal envelope when
the priority lane is empty, to `processing`. It creates or recovers the
database receipt and starts the delivery's only processing attempt. A
continuing priority stream may starve normal jobs. Every job-processing error
fails the delivery and archives the envelope. Known item-local source errors
allow draining to continue; an unexpected model, runner, or runtime processing
failure ends the activation nonzero, leaving later jobs queued for the next
activation.

After recovery and registration, Annals performs one authenticated account
preflight before the activation's first queued dispatch. The preflight does not
claim an envelope, increment its attempts, or start a source delivery. If
authentication is unavailable, the activation ends nonzero and every queued
job remains unstarted. An already processing job is recovered before this
queued-dispatch check.

The activation-long run lock excludes workers. A shorter control lock orders
dispatch, pause, registration, direct enqueue, queued-job priority changes,
interruption, and terminal job disposition. Priority changes apply only to
queued envelopes, preserve their immutable sequences, and are visible to the
next claim. Operator pause allows the current delivery to finish and blocks the
next claim. A durable interrupt targets one named processing job for failed or
skipped archival without itself pausing later dispatch. Deployment maintenance
blocks registration, enqueue, repair, priority changes, and dispatch. Pause and
maintenance are independent; `resume` never removes maintenance.

Recovery never starts a second liaison for a job whose receipt records an
attempt. It may finish durable success already established by that attempt,
such as a conclusively retained duplicate or the job's exact linked
reconciliation. Without such success, it fails the interrupted delivery and
archives the job.

### Bounded retry boundary

Recovery of terminal failures is an explicit retry event, not another pass of
ordinary queue draining. The operator supplies two failed inbox job anchors.
Annals resolves their failed deliveries in ascending `(completed_at, delivery
ID)` order, includes both endpoints, and stores the complete ordered membership
before any retry child runs. Both bounds are mandatory. Because membership is
frozen, a later failure cannot enter an existing event and no command means
"retry every failed job."

Each retry item relates one immutable original failed job and delivery to at
most one fresh child job and delivery. The original envelope stays under
`failed/`, its receipt keeps its one attempt, and its database delivery remains
failed. The child has its own identity and one attempt. Retry provenance is an
explicit integration intent: content-addressed retention may recognize the
same work, but the ordinary fresh-duplicate early return is not taken. The
child can complete the exact reconciliation owned by the original attempt when
that durable record still validates, or it can begin a new examination. It
cannot adopt merely similar history. Its version-6 job receipt carries the
event, event ordinal, original job, and original delivery together, plus the
exact original reconciliation when one is eligible for validation and reuse.

Retry execution uses the normal run and control locks but requires the
operator pause to be set, closing the dispatch gate, the spool to have no
processing job, and deployment maintenance to be absent. It processes children
sequentially in the frozen failure order while ordinary dispatch remains
paused. `resume` refuses an unfinished retry event, which keeps ordinary queued
arrivals from interleaving with its corpus transitions. Start and continue
perform one authenticated account preflight before their first zero-attempt
child claim. A failed preflight halts the event without incrementing a child
attempt or starting a child delivery or model run.

SQLite owns event identity and frozen membership while the spool owns child
envelopes, so their publication cannot be one atomic transaction. A
`preparing` event is the durable recovery marker for that interval. Publication
is idempotent: recovery recognizes an already published child or publishes its
one missing child, but never changes membership or creates a second attempt.
The event becomes `running` during child processing and `completed` after all
items are terminal. A known item-local failure remains one item outcome and
processing advances. An unexpected model, runner, or runtime failure instead
changes the event to `halted`; continuing it considers only not-attempted
members. Interrupting the active child with either operator disposition also
halts the event after recording that child's failed or skipped outcome.

Event reporting joins the frozen original provenance to the current linked
child delivery's lifecycle, result, and error. Item outcomes and aggregates are
derived from that authoritative state rather than duplicated as mutable
counters. The result therefore preserves both sides of the audit trail: why
each original failed and what its bounded recovery attempt did.

## Liaison boundary

The liaison runs in an isolated Codex app-server session. Its pointer prompt
contains the work label and frozen base revision, not the complete work or
repository instructions. Session-scoped tools provide bounded work reading,
corpus browsing, and reconciliation-draft operations. No shell, web, planning,
user-input, or multi-agent tools are exposed.

The default `annals-usage` command owns authentication for that session. It
holds one exclusive lease on the installation's persistent, state-local
`CODEX_HOME` for each real-Codex invocation, and explicitly gives that home to
Codex. Refreshes therefore replace credentials in place under the same lease;
Annals does not copy `auth.json` into a disposable runtime. The dedicated
Codex `config.toml` is private and may select only the file credential store,
so persistent authentication does not import ambient tools or other Codex
configuration into the constrained liaison.

Selecting a custom `[liaison].codex` executable bypasses the wrapper-owned
lease and makes credential serialization the custom runner's responsibility.
The runner still performs its generic authenticated account preflight before
the first queued dispatch.

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
2. requires HEAD to equal the stored base revision;
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
redundant direct assertions.

`revert` loads the target transition, derives its inverse, and applies that
inverse to current HEAD. If a targeted fact has changed incompatibly since the
original commit, the revert fails atomically. Successful reversion is a new
commit and never removes the original.

## Validation and tamper detection

Validation begins with SQLite integrity, foreign keys, schema version, and the
absence of forbidden materialized or JSON-authority storage. It verifies work
digests and tool-artifact hashes, then replays every commit in order through
the production reducer.

For each transition it independently derives the canonical effect set from
the before and after states and compares that set with storage. It reconstructs
typed requests, resolves reconciliations at their recorded bases, and checks
draft, reconciliation, ingestion, change, shake, and revert provenance. Any
effect tampering, skipped revision, invalid transition, or disagreement in
derived semantics fails validation.

## Fresh-state deployment boundary

Normal user deployments quiesce the inbox, back up the supported library,
apply the candidate's additive version-3-to-4 migration when needed, switch the
complete release, validate, and restore the prior operator pause state. A
failed cutover restores the pre-migration backup with the prior release.

The version-3 boundary uses `deploy-user.sh --fresh-state`. The deployer stages
and validates a new empty library and paused spool before touching live state.
It disables activation, pauses dispatch, lets the active delivery finish,
registers all remaining arrivals, and applies maintenance. It then moves the
old library, telemetry ledger, sidecars, and whole spool into one rollback
generation and switches in the staged state.

After candidate and installed validation, a dedicated import operation reads
the archived queued envelopes in lane and immutable-sequence order, preserves
their priority choices, copies their unchanged source bytes into new unstarted
envelopes, and verifies the destination count. Attempted processing envelopes
are terminalized rather than imported for another liaison run. The importer
requires an otherwise fresh destination with both pause and maintenance
active. The deployer clears the operator pause while maintenance still prevents
dispatch, commits its cutover receipt, then removes maintenance and wakes
launchd.

Any failure before that commit restores the previous release selector,
configuration, library and sidecars, spool, pause state, and service. On
success the old generation remains under `backups/generations/` for explicit
recovery.

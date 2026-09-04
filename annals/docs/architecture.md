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
provenance. Schema version 5 adds the immutable Krisis producer-acceptance
ledger and accepted-account feed without changing existing semantic, work,
delivery, or retry history.

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

This keeps historical and current behavior identical by construction. There
is no cache whose agreement must be trusted.

### Query boundary

Bounded graph and search operations benefit from SQL joins and indexes. After
replaying the selected revision, Annals projects that in-memory state into
connection-local temporary concept, edge, and evidence tables. Those tables
exist only for the connection and revision being queried. They are disposable
query acceleration, not library state or authority.

## Source-delivery boundary

A source delivery is distinct from its content-addressed work. Manual commands
and dispatched inbox jobs create ingestion receipts with captured source
metadata and lifecycle status. Several deliveries can select the same work.

Producer acceptance is an earlier, distinct boundary. In a dedicated decisions
library, `inbox accept` binds `(library ID, krisis, decision ID)` to one exact
SHA-256 digest. Annals publishes a complete envelope containing unchanged
account bytes plus producer identity before it commits the immutable acceptance
row. Acceptance starts no delivery or model. Exact replay returns the original
job; different bytes conflict. If publication wins but the database commit is
uncertain, the next identical call reconstructs the acceptance from the
envelope rather than publishing another job.

The decisions-library config contains the expected persistent library ID, and
its spool contains the same durable binding. Acceptance and feed reads require
an explicit config, reject library overrides, and fail closed when either
identity differs. This keeps the primary conversation-export library and its
spool outside the producer boundary.

The version-5 database itself also owns an immutable library kind. Dedicated
decision databases are initialized as `decisions`; ordinary initialization and
every version-3 or version-4 migration produce `general`. Acceptance, feed,
and decision-config dispatch require the decisions kind. Generic work add,
integration, inbox admission, backlog import, and generic dispatch require the
general kind, including when the database is selected directly. A config or
alternate spool therefore cannot reclassify the physical library.

A run using that config verifies the database identity and binds or verifies
the spool before recovery or dispatch; first binding requires an empty spool
and fresh queue index. It never registers `incoming/` files,
and every non-retry envelope must carry a valid Krisis producer receipt whose
digest and job metadata match its already committed acceptance. Direct work
add or integration and generic register, enqueue, or backlog-import commands
reject the decisions config; generic inbox admission also rejects any spool
that already carries the decision-library binding. These local role and
routing checks are not authentication against the operating user; the general
source and inbox behavior is otherwise unchanged.

The accepted-account feed is a read-only projection of immutable acceptance
rows. A watermark freezes a committed sequence prefix. Pages are ascending and
strictly after either an earlier watermark or an item cursor, remain fixed to
the requested watermark, and keep an empty-page cursor unchanged. The feed
contains bounded decision projections and one source anchor, never raw account
Markdown or general-library content, and stores no consumer acknowledgement.

The filesystem inbox separates admission from dispatch. Registration moves a
settled source into `queued/JOB_ID/material`, assigns an immutable monotonic
sequence, and writes an unstarted normal-lane job receipt. Direct enqueue
copies explicitly selected files into complete unstarted envelopes, leaves the
originals unchanged, and can select the priority lane without passing through
settling admission. Before copying, enqueue verifies that the copy would leave
the configured storage reserve available on the spool filesystem. Dispatch
finishes any active job, then moves the
lowest-sequence priority envelope, or the lowest-sequence normal envelope when
the priority lane is empty, to `processing`. It creates or recovers the
database receipt and starts the delivery's only processing attempt. A
continuing priority stream may starve normal jobs. Every job-processing error
fails the delivery and archives the envelope. Known item-local source errors
allow draining to continue; an unexpected model, runner, or runtime processing
failure ends the activation nonzero, leaving later jobs queued for the next
activation.

Before each zero-attempt claim, the inbox storage gate checks bytes available
to the Annals user on both the library and spool filesystems. A closed gate
leaves the next envelope queued with attempts zero and no delivery record, and
the ordinary activation exits successfully. It creates no pause marker: the
next scheduled or explicit activation measures again and resumes automatically
when both locations satisfy the reserve. A probe failure also prevents the
claim but ends the activation nonzero with `storage_probe_failed`. Recovery of
an already processing envelope precedes this new-claim gate.

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
The same storage gate is checked before each queued retry child claim. A closed
gate halts the attended event with `insufficient_storage`; an unreadable gate
halts it with `storage_probe_failed`. In either case the child remains queued
and unattempted, and the operator uses `retry continue` after correcting the
condition.

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

The liaison runs as a Nucleus job backed by an isolated Codex app-server
session. Its pointer prompt
contains the work label and frozen base revision, not the complete work or
repository instructions. Session-scoped tools provide bounded work reading,
corpus browsing, and reconciliation-draft operations. No shell, web, planning,
user-input, or multi-agent tools are exposed.

Annals registers the exact nine-tool contract with Nucleus, submits a
deterministically identified job, and services Nucleus's durable requester
mailbox. The base and developer instructions, pointer prompt, model, reasoning
effort, lack of builtin shell/web access, and tool schemas are the same as the
former in-process runner contract. A repeated ambiguous submission carries
byte-identical request content. Tool results are cached before transmission,
so retry after an ambiguous transport failure never executes an Annals backend
operation twice. Annals continues to determine success from the durable
recorded reconciliation, not from the model's final message.

Nucleus exclusively owns Codex process isolation, persistent authentication,
credential refresh, and serialization across jobs and account operations.
Annals neither reads nor sets `CODEX_HOME` and has no direct-runner fallback.
`[liaison].nucleus_socket` optionally selects a nonstandard Unix socket; its
default is Nucleus's current-user socket. Before the first new queued attempt,
Annals asks Nucleus for an authenticated account preflight and may wait up to
30 seconds for Nucleus's authentication lease. Failure leaves the envelope
queued with attempts zero and no source-delivery record.

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

## Fresh-state deployment boundary

Normal user deployments quiesce the inbox, back up the supported library,
apply the candidate's additive migration through version 5 when needed, switch the
complete release, check its commands, library statistics, and inbox state, and
restore the prior operator pause state. A
failed cutover restores the pre-migration backup with the prior release.

The version-3 boundary uses `deploy-user.sh --fresh-state`. The deployer stages
an initialized empty library and verifies its paused spool before touching live
state. It disables activation, pauses dispatch, lets the active delivery
finish, registers all remaining arrivals, and applies maintenance. It then
moves the old library, its sidecars, and whole spool into one rollback
generation, switches in the staged state, and checks the installed library
statistics.

After candidate and installed checks, a dedicated import operation reads
the archived queued envelopes in lane and immutable-sequence order, preserves
their priority choices, copies their unchanged source bytes into new unstarted
envelopes, and verifies the destination count. Attempted processing envelopes
are terminalized rather than imported for another liaison run. The importer
requires an otherwise fresh destination with both pause and maintenance
active. The deployer clears the operator pause while maintenance still prevents
dispatch, switches the exact-release Clockwork binding, commits its cutover
receipt, then removes maintenance for the next activation.

Any failure before that commit restores the previous release selector,
configuration, library and sidecars, spool, and pause state. It restores the
exact prior Clockwork definition only when its binding was enabled, or the
legacy LaunchAgent, never both; a prior absent or disabled binding stays
disabled without transient activation. Every selected definition is compared
field for field with the complete relevant Annals release before Annals
disables or replaces it; unknown same-key state is left untouched. On
success the old generation remains under `backups/generations/` for explicit
recovery.

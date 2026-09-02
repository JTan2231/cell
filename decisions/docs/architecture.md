# Architecture

## Boundary

Decisions owns the durable answer to “what decision candidates were found,
reviewed, frozen, and sent?” Its SQLite database is the authority for runs,
candidates, source anchors, reviews, lifecycle events, snapshots, and delivery
attempts. Nucleus
job state is execution evidence, not decision truth. A validated classification
receipt is durable domain input and remains usable if later Nucleus observation
or runtime reporting fails.

The source boundary is the public `conversations` Rust library. Decisions never
reads Codex JSONL or SQLite files. The continuous path asks App Server for one
exact completed turn: normalized user/assistant messages plus content-free
references and counts for successfully completed, nonempty `fileChange` items.
Paths, diffs, commands, reasoning, tool output, and approval payloads do not
cross that boundary. A hook `session_id` is only a resolution hint; it is not
assumed to equal the canonical thread ID. Conversations checks exact turn
membership across the visible lineage and returns only one unique match.

The scheduled reconciliation path enumerates normalized completed-turn
activity for root interactive tasks only. It excludes `exec` and subagent
threads, uses item timestamps or the containing turn start—never thread update
time or file mtime—as authority time, and fails closed on incomplete or
ambiguous source. Reconciliation only enqueues stable turn correlations; it
does not classify.

The Codex `Stop` hook is a wake signal, not a transcript or decision boundary.
It synchronously and idempotently records only `session_id` plus `turn_id` with
a three-second timeout, then returns empty JSON to Codex. It discards the rest
of the event, including transcript path, working directory, model, permission
mode, and latest assistant message. A launchd worker processes one durable
observation at a time every 60 seconds. Background hook execution is not used:
Codex may cancel background hooks at session end and permits them to finish out
of order.

Deployment records a write-once observer baseline after schema migration and
release staging, but before the live hook, command, plists, or either service
can run. The default activation stores the next whole Unix second, so every
authority item timestamped in the cutover second is conservatively excluded.
Processing, reconciliation, and daily projection require that baseline. An
authority user message before it is permanently ineligible, so first-day
coverage begins at the later of local midnight and the stored baseline.
Redeployment and reinstall do not advance the baseline and cannot import
earlier messages.

## Enacted decision semantics

A decision is an attributable transition from practical openness to operative
settlement. It can adopt, reject, forbid, intentionally defer, delegate,
reopen, or supersede a material choice. Assistant context can resolve a user's
“yes,” “this works,” or “do that,” but assistant-only recommendations, plans,
questions, status or implementation reports, tool approvals, silence, copied
subagent prompts, and file activity are not authority.

The continuous product intentionally projects *enacted* decisions: the same
completed root turn must contain at least one authoritative user message and at
least one successfully completed, nonempty App Server file-change item. The
effect makes the turn eligible for examination; it does not prove what was
decided and is never cited as authority. A turn with writes but no explicit
user settlement receives durable `no_decision` coverage. A user settlement
without a same-turn completed file change is outside this enacted projection.

## Continuous classification

The first Nucleus invocation is a bounded level-0 slice, not the whole turn or
thread. It contains every eligible user authority message in the completed
turn, the immediately preceding assistant proposal needed to interpret those
authorities when one exists, at most the final assistant result from the
completed turn, and only the number of completed file-change effects. The
classifier must return exactly one `decision` or `no_decision` verdict for every
authority source. It can instead return a validated `needs_context` result when
a referential settlement cannot be resolved from that slice. That advances the
observation once to a level-1 scope with normalized prior conversation context;
authority remains restricted to the original turn. There is no further
expansion and no parallel whole-thread fan out.

Source resolution precedes classification. A turn that is not yet complete or
visible remains queued with a future retry time. Both regular and projection
selection resume an existing `processing` row first, skip queued work whose
retry time has not arrived, and order ready queued work by retry-ready time. A
deferral therefore yields to other ready observations while preserving one
serial processor. A merely unfinished turn can later resolve and must never be
treated as permanently unavailable.

Jobs use `workspaceAccess=none`, no built-in local execution or web search, no
launch context, and the immutable `decisions/turn-classification/1` toolset.
Nucleus durably retains each exact request prompt and tool exchange in its
private local database and does not prune them automatically. The retained
request contains the bounded level-0 slice, or the normalized full thread
prefix when level 1 is required; Decisions stores only its request digest and
validated domain result. Nucleus owns retention and recovery of those request
records.

Each scope starts with one bounded Nucleus job. Only a positively observed
terminal failure permits retry 1 and retry 2, for at most three attempts in
that scope. The observer processes at most one observation synchronously, so
parallel invocations are not the throughput mechanism. An uncertain admission,
observation timeout, requester restart, or post-result transport failure stays
correlated to the same observation, scope, job, and durable receipt.

Before admission, Decisions stores the deterministic request digest—not the
request or transcript—and fences that intent with its private per-database
operation lock. It writes the exact bounded tool-result bytes and a text-free
validated candidate DTO before acknowledgement. Restart replays that receipt.
A successful receipt and a positively observed terminal failure are
first-writer-wins outcomes: success prevents retry, while committed failure
rejects a late success. A terminally failed observation is not automatically
requeued and blocks its daily projection until deliberately repaired. Explicit
`observe retry` increments that observation's attempt epoch while retaining
every earlier job and receipt as audit history.

An explicit source-unavailable recovery is narrower than retry. After the exact
Stop-hook correlation has been proven permanently unavailable,
`observe abandon OBSERVATION_ID --source-unavailable` waits for the same serial
processing lock and can close only a previously observer-deferred pending
level-0 row whose source remains entirely unbound and which has no job,
authority, verdict, or candidate. The transaction records `complete` /
`not_eligible` with the fixed `conversation_source_abandoned` marker, stores no
free-form reason, emits no lifecycle event, and leaves the baseline unchanged.
An exact repeat is idempotent recovery from uncertain command completion. Any
bound, processing, failed, other completed, or merely unfinished source is
refused; if later completed-root reconciliation discovers an abandoned
correlation, it fails closed rather than changing that audited state.

The managed tool may authorize candidates only from supplied user source IDs.
Each candidate cites a unique exact authority span; stable IDs derive only from
host, canonical item ID, and that byte span. Fork provenance, model prose,
confidence, disposition, effect count, and observation identity cannot change
candidate identity. Duplicate authority spans are merged only after validated
classification, conflicting meanings fail closed, and low-confidence output
is discarded.

Statements and rationales pass a deterministic disclosure check before
storage. Controls, secrets or credentials, account/email identifiers, paths,
source identifiers, prompt/tool traces, and raw transcript fragments are
rejected rather than redacted. The complete observation transaction records
positive candidates and negative authority verdicts in Decisions SQLite; model
output or Nucleus completion alone is not domain truth.

## Lifecycle event stream

Schema version 3 adds an append-only local consumer stream. The first durable
admission of a stable candidate appends one immutable version-one
`decision_admitted` envelope in the same transaction as candidate sources,
authority verdicts, observation attachment, and observation completion. A
duplicate classification of the same immutable candidate does not create a
second admission event. Every committed confirm or dismiss review appends one
`decision_reviewed` envelope in the same transaction as its append-only review
row, current candidate state, and digest invalidation. Event failure rolls back
the owning domain transition.

`events watermark` returns an opaque current position. `events read` returns a
bounded page strictly after a supplied cursor, with a cursor for every event so
a consumer can commit one copied event and its position atomically. Replaying a
cursor is deterministic and expected after uncertain consumer persistence.
Decisions keeps no consumer acknowledgement and does not block on downstream
availability. The cursor is transport state, not decision identity or
authority.

The immutable envelope contains normalized candidate meaning, lifecycle state,
and stable authority/context source anchors. It contains no transcript text,
working directory, file paths, diffs, commands, reasoning, approval payloads,
tool output, hook body, prompt, or delivery data. Schema migration backfills
retained admissions and reviews; a new consumer that should ignore history
records the current watermark as its activation cursor.

## Reconciliation and daily projection

The observer LaunchAgent invokes one `observe process` every 60 seconds. A
healthy day therefore classifies turns shortly after completion. `observe
reconcile` is an idempotent safety net for a missed or untrusted hook: it scans
post-baseline completed effectful root turns and enqueues them without model
work. At 09:00, `daily run --scheduled` records a durable
`coverage_cutoff_at`, reconciles prior-date turns completed at or before that
instant, and drains only missed observations before creating its projection.
Reconcile and drain repeat to a fixed point before the build captures the
SQLite-rowid `observation_admission_watermark`; a hook admitted after that
watermark cannot race into the immutable run and remains available to a later
build.
The routine morning path performs no model work; exceptional catch-up remains
serial and can invoke Nucleus.

Projection completeness is calculated from durable observations and verdicts,
not from successful jobs alone. Any known in-window turn completed by the
run's cutoff that is queued, processing, failed, ambiguous, incomplete, or
otherwise uncovered prevents normal or all-clear delivery. A run records the
exact covered window, whose start is
`max(local_day_start, observer_baseline_at)`, its completion cutoff, and the
stable candidates whose authority falls inside it. A turn whose authority is
in that date but which completes after the cutoff can enter a later manual
rebuild of that date. An already accepted scheduled delivery is not
automatically amended, and the late turn is not carried into a different
authority day.

## Legacy scan recovery

Schema versions 2 and 3 preserve version-one runs as `legacy_scan` records together
with their original whole-thread Nucleus correlations and receipts. A matching
interrupted legacy build resumes its deterministic current attempt. Only a
positively observed terminal failure permits its two retries; uncertain
admission remains resumable and never authorizes a concurrent job.

`daily abandon` remains only for a legacy `building` or `abandoning` run whose
source can no longer be reproduced. It fences new legacy work, resolves every
correlated job before cancellation, requires observed terminality, and then
marks the run failed. An admitted intent with no observable job is restored to
`building` only when the exact stored snapshot is unchanged. This legacy daily
abandonment never deletes, requeues, or supplies a verdict for a continuous
observation.

## Delivery

A preview freezes subject and plain-text body for the run's current content
revision. Any review increments affected revisions. Send records the exact key
before invoking Email and always re-reads that delivery's frozen snapshot.
Manual keys are unique and never consume the scheduled occurrence. Scheduled
keys are `codex-decisions-daily/YYYY-MM-DD`, where the date is the local 09:00
occurrence rather than the report date.

If a manual attempt is pending or failed for the same run revision, the next
manual send reuses that delivery's frozen body and key so acceptance before a
lost local database update cannot duplicate mail. After acceptance, another
explicit manual send creates a new delivery and key.

Normal and all-clear email are possible only from a complete run. A source,
timestamp, Nucleus, protocol, classification, or database failure marks or
aborts the build and stops delivery. Upstream Conversations, Nucleus, and Email
error bodies are not copied into Decisions state or scheduled logs; operators
inspect those products' diagnostics separately. Email owns Resend submission;
Gmail owns receipt.

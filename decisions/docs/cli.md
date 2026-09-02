# CLI

`--date` always means the local calendar day being summarized. Without it,
build, preview, and manual send select yesterday.

```sh
decisions doctor
decisions daily build [--date YYYY-MM-DD]
decisions daily preview [--date YYYY-MM-DD]
decisions daily send [--date YYYY-MM-DD]
decisions daily abandon [--date YYYY-MM-DD]
decisions daily run --scheduled
decisions observe ingest
decisions observe activate [--at UNIX_SECONDS]
decisions observe process
decisions observe status [--date YYYY-MM-DD]
decisions observe reconcile [--date YYYY-MM-DD]
decisions observe abandon OBSERVATION_ID --source-unavailable
decisions observe retry OBSERVATION_ID
decisions events watermark
decisions events read --after OPAQUE_CURSOR [--limit 1..1000]
decisions show DECISION_ID
decisions review confirm DECISION_ID
decisions review dismiss DECISION_ID
```

`daily build` records a durable `coverage_cutoff_at`, first reconciles selected-
date turns completed at or before that instant, and drains only observations
missed by the continuous path. Reconciliation and draining repeat to a fixed
point, then the build captures `observation_admission_watermark`, the greatest
durable observation row admitted to this projection. Later hook rows remain for
a later build instead of racing the frozen result. The command then creates an
as-of projection from durable observation verdicts. On a healthy installation
the queue is already drained,
so this performs no routine morning model work. Exceptional catch-up can invoke
Nucleus serially. Any failed, unresolved, ambiguous, incomplete, or
pre-activation-ineligible source known by that cutoff is handled fail closed; a
failed in-window observation blocks the projection. Coverage starts at the
later of local day start and the write-once observer baseline. A turn that
completes after the cutoff can enter a later manual rebuild of its authority
date, but an accepted scheduled delivery is not automatically amended and the
turn is not carried into another date. Build never sends.

Version-one `legacy_scan` runs remain readable and resumable with their original
requester/job IDs and request digests. Only a positively observed terminal
classifier failure permits the next of two deterministic automatic retries,
for at most three attempts; uncertain admission remains on the same attempt.
The private per-database operation lock still serializes legacy build admission
against abandonment.
`daily preview` reads the latest complete
run, freezes its current revision, and prints the exact subject and body without
network access. `daily send` invokes Email synchronously. It reuses the frozen
key and body of the latest pending or failed manual delivery for that run
revision; after acceptance, another explicit send creates a fresh ad-hoc key.

`daily abandon` remains the explicit recovery path for a legacy `building` or
`abandoning` run whose source can no longer be reproduced. It blocks new work,
cancels every nonterminal correlated Nucleus job, requires observed
terminality, then marks the run failed. If Nucleus cannot prove terminality,
the date remains blocked for safe retry. An admitted intent with no observable
job is restored to `building` only when its exact snapshot is unchanged. This
command never deletes, requeues, or overrides continuous observations.

`daily run --scheduled` is the LaunchAgent entry point. At or after 09:00 it
uses today's occurrence and yesterday's report date; before 09:00 it uses the
previous occurrence. A repeated invocation returns an already accepted
delivery or retries the same frozen body under the same key.

## Continuous observation

`observe ingest` is the synchronous Codex `Stop` hook interface. It reads one
event object from standard input, requires the `Stop` event and stable
`session_id`/`turn_id` fields, durably upserts that correlation, and writes `{}`
for Codex. It does not inspect Conversations or invoke Nucleus, and it never
persists the prompt, latest assistant message, transcript path, working
directory, or other event body.

`observe activate` records the earliest eligible authority time exactly once.
Without `--at`, it stores the next whole Unix second, conservatively excluding
authority items timestamped in the activation second. Ordinary deployment
calls it after candidate migration and release staging but before the live
plists, hook, public command, or either service is published. `--at` stores the
specified Unix second exactly and is only for isolated tests or explicit
recovery. If a baseline already exists, the stored value is returned unchanged
even when a different `--at` is given.

`observe process` requires an activation baseline and processes at most one
observation synchronously. It resumes an in-flight processing observation
first. Otherwise it skips queued rows whose `next_attempt_at` has not arrived
and orders ready work by its retry-ready time, so a deferred source yields to
other ready observations instead of monopolizing the worker. It resumes
in-flight Nucleus work with its durable correlation and receipt rather than
starting a parallel attempt. The completed turn is resolved from the hook's
session hint and exact turn ID through Conversations; zero or multiple matches
fail closed. A turn is eligible only when it is a completed root interaction,
contains an authoritative user message at or after the baseline, and has at
least one completed nonempty App Server `fileChange` item. File activity only
selects the scope—it never supplies decision authority.

A source that is not yet complete or visible remains queued with a future retry
time. This is source-resolution deferral, not a classifier attempt or terminal
failure. A merely unfinished turn remains eligible for later resolution and
must never be abandoned as unavailable.

The first classifier invocation receives a bounded level-0 slice: every
eligible current-turn user authority, the immediately preceding assistant
proposal when one exists, at most the current turn's final assistant result,
and only a count of completed file-change effects. It does not receive every
message in the turn or thread. A validated `needs_context` result may expand
exactly once to normalized prior context from the same conversation; it does
not fan out into parallel whole-thread jobs.
Each scope retains the bounded terminal-only retry and durable requester-receipt
rules. The classifier must return one `decision` or `no_decision` verdict for
every supplied user authority message. An uncertain job remains resumable;
terminally failed observations are not automatically requeued and block their
daily projection until repaired deliberately. After diagnosing and correcting
the cause, `observe retry OBSERVATION_ID` explicitly requeues one terminally
failed observation, increments its attempt epoch, and retains its prior jobs
and receipts as audit history.

`observe abandon OBSERVATION_ID --source-unavailable` is a separate, explicitly
confirmed recovery for one Stop-hook correlation that has been proven
permanently unavailable. It waits for the serial observation-processing lock
and accepts only a level-0 queued row previously deferred as a pending
`TurnNotFound`-shape source: `next_attempt_at` is set,
`source_not_completed_at` is unset, and no source, classification job,
authority, verdict, or candidate has been bound. It atomically records
`complete` / `not_eligible` with the fixed
`conversation_source_abandoned` audit marker. It stores no caller-provided
reason, creates no verdict, candidate, or lifecycle event, and does not change
the observer baseline. Repeating the exact successful command is idempotent for
uncertain command completion; every other state fails closed. If completed-root
reconciliation later finds that exact correlation, reconciliation fails closed
instead of silently overriding the explicit recovery.

`observe reconcile` is the missed-hook safety net. It discovers completed
effectful root turns in the selected date's post-baseline coverage, idempotently
upserts their correlations, and performs no classification. `observe status`
reports `observer_baseline_at` and queued, processing, complete, and failed
counts. With `--date`, it counts observations whose admitted authority falls in
that local date and conservatively includes unresolved failures that are not yet
scoped to an authority time. The observer LaunchAgent calls `observe process`
every 60 seconds; these commands do not provide a parallel worker mode. Queued
counts include source rows waiting for their retry time, so a nonzero queue can
coexist with `No observation ready`.

## Lifecycle events

`events watermark --json` returns the current end of the append-only
`decisions.lifecycle` stream as an opaque cursor. Every version-one consumer
stores this current cursor before it begins polling. Historical replay is not
exposed by this interface.

`events read --after CURSOR --json` returns immutable version-one envelopes
strictly after the cursor in append order. `--limit` defaults to 100 and accepts
1 through 1000. The page contains `after_cursor`, `next_cursor`,
`watermark_cursor`, `has_more`, and an `events` array. Every array item has its
own opaque `cursor` and nested `event`, allowing a consumer to durably copy and
advance one event at a time. An empty page leaves `next_cursor` equal to the
input cursor. Reusing a cursor is safe and returns the same committed prefix.

Consumers own their cursor, deduplication, retry, retention, and domain success;
Decisions records no acknowledgement. Invalid, noncanonical, or modified
cursors fail with `event_cursor_invalid`; positions beyond the current stream
fail with `event_cursor_ahead`; invalid limits fail with `event_limit_invalid`.
Do not parse or manufacture cursors. See the installed
`decisions.lifecycle.consume` contract for the exact envelope fields and
privacy boundary.

Use `--json` for structured output. `--database` and
`DECISIONS_DATABASE` select a nonstandard database. `--email-binary` and
`DECISIONS_EMAIL_BINARY` are operational/test overrides.

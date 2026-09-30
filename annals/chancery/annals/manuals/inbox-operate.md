# Operate the Annals inbox

Use this operation to inspect or operate one configured inbox. Read the
[inbox feature](inbox.md) for queue, attempt, control, retry, status, and recovery
semantics. Use [installation](installation.md) for program or scheduler changes.
These commands do not authorize receipt editing, archive moves, unbounded
retry, or storage cleanup.

## Select and inspect

1. Select the registered library with `annals library NAME inbox ...`, or use
   its explicit config. Confirm the expected database, library ID, kind, and
   spool. A decisions config permits only producer originals and their retries.
2. Run `annals inbox status`. Inspect the active and next jobs, worker state,
   storage readiness, operator pause, maintenance, and terminal counts. Reads
   require prepared state and do not initialize or recover it.
3. Stop if identity, kind, locks, storage probes, or initialized state cannot
   be verified. Use the supported setup or recovery operation. Preserve the
   complete error instead of inferring readiness from processes.

Status, retry preview, and event reports are read-only. They require read
access to selected config, library, prepared SQLite sidecars, and applicable
spool locks and receipts. Registration/enqueue require a configured general
library and spool. Dispatch additionally requires authenticated compatible
Nucleus and complete Bazaar prompt selection.

## Admit general-library files

Register settled top-level arrivals:

```sh
annals inbox register
annals inbox register --settle-seconds 60
```

Registration moves eligible source files into complete queued envelopes. It
starts no source delivery or model attempt. Pause permits registration;
deployment maintenance prevents spool mutation. Hidden entries, directories,
and `.part` files are ignored. An arrival still settling waits for a later scan.

Copy explicitly selected regular files into the spool:

```sh
annals inbox enqueue FILE...
annals inbox enqueue --priority FILE...
```

Enqueue leaves original files unchanged and rejects a copy that would cross
the configured reserve. Check the returned job IDs, immutable sequence,
priority, queued count, and next job. Keep producer-only decisions admission
on [the exchange inlet](decision-account-exchange.md).

## Change priority or dispatch

Change only named queued ordinary jobs:

```sh
annals inbox prioritize JOB_ID...
annals inbox deprioritize JOB_ID...
```

Verify the requested lane and next job. These commands do not renumber jobs,
preempt an active job, or control retry children. A terminal or processing ID
is an error.

Run one activation when source examination and automatic application are
intended:

```sh
annals inbox run
annals inbox run --stop-on-failure
```

The runner registers arrivals, recovers an interrupted envelope, and drains
sequentially while controls permit it. New work can invoke Nucleus, consume
allowance, and apply a reconciliation immediately. Fresh exact-byte duplicates
stop at retention. Every claimed job has one attempt.

Inspect the run report's attempted, applied, recorded, duplicate, failed,
skipped, remaining, and gate facts. Manual ordinary draining can continue past
item-local failures. `--stop-on-failure` returns nonzero before a successor
claim after the first failure; installed scheduled runners use this option.
An unexpected model or runtime failure also ends the activation. Preserve any
Annals result already recorded before a later runtime failure.

Low storage leaves the next job queued with attempts zero and no delivery
record. An ordinary activation exits successfully and rechecks on the next
activation. A failed probe or authenticated preflight exits nonzero with the
job unattempted. Report affected paths and capacity. Do not clear user data or
change the reserve without explicit user consent for the exact target and scope.

## Pause, resume, or interrupt

Prevent successor claims before stopping an exact active job:

```sh
annals inbox pause
annals inbox status
annals inbox interrupt JOB_ID --as failed --reason 'Operator context'
```

Use `--as skipped` only when that disposition is intended. Pause permits the
active job to finish and blocks later claims. Interruption alone does not pause
successors. It can conflict as too late when a terminal outcome or recorded
reconciliation already exists. A skipped receipt corresponds to a failed
source delivery with `inbox_job_skipped`.

Verify the named terminal archive and source-delivery outcome. Resume ordinary
dispatch only when that operator pause should be removed:

```sh
annals inbox resume
```

Resume starts no worker and clears only the operator pause. It cannot clear
maintenance, unfinished retry events, or a Clockwork failure halt.

## Recover a bounded failed-delivery interval

1. Run `annals inbox retry preview --from FAILED_JOB --through FAILED_JOB`.
   Both anchors are required and inclusive in failed-delivery completion order.
   Use the same ID twice for one failure. Inspect the whole frozen proposal.
2. Correct a pre-retention source failure and deliver it as new input. Do not
   select skipped jobs, already selected originals, changed or missing material,
   or an interval containing an ineligible member.
3. Run `annals inbox pause`, then inspect status. Wait for paused quiescence:
   no processing job, no unfinished retry event, and no maintenance.
4. Run `annals inbox retry start --from FAILED_JOB --through FAILED_JOB`.
   Add `--reason TEXT` only for trimmed, nonempty context of at most 1,000
   characters. Start preserves every original and creates fresh linked children.
5. Read `annals inbox retry status EVENT_ID`. Add `--details` to verify the
   complete original-to-child mapping. Inspect item outcomes and remaining work;
   exit zero alone does not mean every member succeeded.
6. Correct an actionable halt, then run `annals inbox retry continue EVENT_ID`
   in the same paused, quiescent, non-maintenance state. Continue advances only
   unattempted members and never gives a failed or skipped child another attempt.
7. Verify `completed` and the durable outcomes. Resume the operator pause only
   when ordinary dispatch is intended.

Start freezes the exact selection; later failures never enter it. Missing or
changed archive material rejects the whole window. Only failures with retained
work identity are eligible. There is no retry-all or open-ended event.
Insufficient or unreadable storage and failed preflight halt attended retry
without claiming the next child. A failed unexpected attempt or interruption
also halts the event. Ordinary scheduling never continues it.

A later bounded event can select a failed child to make another explicit link
in the retry chain. It cannot rewrite the original failure. Use event status
as the accounting authority; `lately` and Annals Usage report each delivery
separately.

## Recover interrupted processing and scheduling

Run the supported runner or retry continue command. Recovery finishes durable
success when possible, otherwise fails and archives the interrupted job. It
never invokes a second liaison for an attempted receipt, adopts an unrelated
reconciliation, or moves a terminal envelope back to queued. Verify the exact
job and linked Annals domain result after recovery.

A scheduled failure ends the current batch. Clockwork permits later activations
until the shared service-health threshold establishes a halt. By default, five
consecutive failed read-only checks, at least 60 seconds apart, halt the binding
and make its alert eligible together. Healthy or inactive checks clear a pending
episode. Failed deliveries and attended retry events retain their explicit
recovery rules.

For an established scheduled halt, inspect the matching incident:

```sh
clockwork incident list annals/inbox
clockwork incident show INCIDENT_ID
```

Use `annals/decisions-inbox` for that binding. Diagnose the Annals delivery and
its recovery separately. Resume the binding only after explicit approval for
that incident:

```sh
clockwork binding resume KEY INCIDENT_ID
```

Annals resume, retry, dependency recovery, definition switches, and deployment
do not release the Clockwork halt. Scheduling continuation neither retries a
failed delivery nor clears an Annals control. Verify the binding and product
state separately. Low storage, pause, maintenance, and an empty queue are
normal readiness conditions; failed storage probes and authentication are
abends. Existing archives are history, not new incidents.

## Private state and command usage

Keep source envelopes, library state, logs, backups, and Nucleus context private.
This operation grants no publication, deletion, credential, or service-restart
authority. A request to continue authorizes its documented effects only.

CLI usage recording requires nonempty `CODEX_THREAD_ID`. Chancery records
command identity, time, and thread ID, not arguments, output, or outcomes.
Internal calls are excluded. Recording errors preserve command results.

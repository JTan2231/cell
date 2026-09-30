# Inbox admission, dispatch, and recovery

The Annals inbox is a durable filesystem spool for sequential source imports.
Annals owns envelopes, source-delivery outcomes, priority, interruption,
bounded retries, and domain recovery. External scheduling wakes a runner;
Annals has no resident inbox daemon or internal scheduler. Use
[inbox operations](inbox-operate.md) for procedures and
[installation](installation.md) for platform scheduling and cutover.

A registered name selects the library's catalog identity, config, database, and
spool as `annals library NAME inbox ...`. New named libraries have private
configs and spools but no schedule. Background configurations keep the expected
persistent library ID, so replacing a database cannot silently rebind dispatch.
Read [libraries](libraries.md) for selection and [interpretation](work-integrate.md)
for frozen instructions and model authority.

## Interfaces and configuration

```text
annals inbox register [--settle-seconds SECONDS]
annals inbox enqueue [--priority] FILE...
annals inbox prioritize JOB_ID...
annals inbox deprioritize JOB_ID...
annals inbox run [--settle-seconds SECONDS] [--stop-on-failure]
annals inbox pause
annals inbox resume
annals inbox interrupt JOB_ID --as failed|skipped [--reason TEXT]
annals inbox retry preview --from JOB_ID --through JOB_ID
annals inbox retry start --from JOB_ID --through JOB_ID [--reason TEXT]
annals inbox retry status [EVENT_ID] [--details] [--limit N]
annals inbox retry continue EVENT_ID
annals inbox status
```

All commands except `inbox retry status` require an `[inbox]` config section
with `root`; retry-event reports are durable library reads and can be selected
with `--library` alone. The optional config key `settle_seconds` defaults to 60;
the `register` and `run` flags override it. A zero settling interval is allowed.
`minimum_available_bytes` defaults to `7_000_000_000` and sets the storage
reserve required before a new inbox claim; zero disables that gate.
`inbox status`, `inbox retry preview`, and `inbox retry status` are read-only.
Status and accepted-document reads open the existing control lock read-only
and take a shared lock against spool writers. Setup creates that lock. Reads
never create it. Worker-lock probes require only read access and report access
errors instead of reporting the worker unlocked.

## Dedicated decision intake

A configured decisions library admits complete documents only through its
Krisis producer inlet. Read [decision document exchange](decision-account-exchange.md)
for its explicit identity, exact key-and-byte replay, accepted envelope, and feed.
Acceptance publishes one complete unstarted envelope and immutable event; it
starts no source delivery or model attempt.

Decision config rejects direct `work add`, direct `integrate`, generic enqueue,
ordinary registration, and backlog import. Its dispatcher binds or verifies
the dedicated spool, ignores `incoming/`, and accepts only producer originals
or their explicit retry children. Decision-config commands require a
`decisions` database. Generic admission and dispatch require `general`.
Changing config, spool, name, instructions, or direct path selection cannot
bypass the immutable kind. The ordinary admission rules below apply only to
a general library without `[decision_feed]`.

## Register sources

`inbox register` moves every settled file into a durable queued job without
processing it. Each file moves, without changing its basename or bytes, into
`queued/JOB_ID/material/` beside an operational `job.json` receipt. The
receipt has state `queued`, attempts zero, and an immutable monotonic sequence.
Registration creates no database source-delivery record. Human output reports
the registered jobs; JSON includes each assigned job ID and sequence with
`priority` set to `normal`.

## Enqueue sources

`inbox enqueue` copies each named regular file into a new durable queued
envelope and leaves the original unchanged. It bypasses `incoming/` and the
settling interval. The envelope becomes dispatchable only after its material
and receipt are complete, so dispatch cannot start during a partial copy.
Files receive immutable monotonic sequences in argument order and enter the
normal lane unless `--priority` selects the priority lane.

The copy is rejected with `insufficient_storage` when its size would leave
less than `minimum_available_bytes` available on the spool filesystem. The
result reports the spool root, selected priority, registered count, each job's
ID, sequence, and priority, the total queued and priority-queued counts, and
the next job. Like registration, enqueue starts no source delivery.

## Change queue priority

`inbox prioritize JOB_ID...` moves the named queued jobs to the priority lane;
`inbox deprioritize JOB_ID...` moves them to the normal lane. Both operate
only on jobs that are still under `queued/`, hold the queue-control lock for
the mutation, and leave each job ID and immutable sequence unchanged.

Argument order therefore does not reorder jobs; an older normal job moved to
priority can precede newer priority jobs. Requesting the lane a job already
has is an idempotent success. The result reports the spool root, requested and
changed counts, selected priority, requested jobs, the priority-queued count,
and the next job.

Naming a processing or terminal job, or an unknown job ID, is an error rather
than a request to alter history. A retry child is controlled by its retry
event and cannot be prioritized or deprioritized independently.

## Run the inbox

`inbox run` takes the activation-long spool lock, performs the same
registration phase, and drains jobs sequentially while processing is allowed.
After recovery and registration, it checks available storage on the library
and spool filesystems before each queued claim. If either location is below
the configured reserve, the job stays queued with attempts zero and no
delivery record, `stopped_for_low_space` is true, and the activation exits
zero.

No pause is created; a later scheduled or explicit activation measures again
and continues automatically once both locations are ready. Failure to measure
storage exits nonzero with `storage_probe_failed` and also leaves the job
unattempted. An already processing job is recovered before this gate.

Low storage stops dispatch without granting cleanup authority. Preserve queued
work and report affected paths and available bytes. No liaison, operator model,
or agent can delete, truncate, rotate, prune, move, compress, overwrite, or
otherwise clear user data, or lower or disable the reserve, as remediation
without the user's explicit consent for the exact target and scope. A request
to run, continue, retry, update, or deploy authorizes only its documented effects.
Scheduled rechecks observe capacity and can resume ordinary work when the
reserve becomes available.

The gate applies to ordinary and retry-child claims. The enqueue headroom check
separately applies to explicit spool copies. A gated item submits no liaison
job. The gate is not a host-wide lock: it does not itself prevent manual
integration, independent Nucleus work, or deployment. Actual exhaustion can
still make shared-storage writes fail, including release staging, backup,
migration, state, or logs. An unreadable probe is distinct from measured low
space and can make deployment inspection fail with `storage_probe_failed`.

When storage is ready, `inbox run` performs one authenticated account
preflight before its first queued dispatch. The preflight does not claim a job,
increment attempts, or start a source delivery. If it fails, `inbox run` exits
nonzero while the next envelope remains under `queued/` with attempts zero and
no database delivery record. Annals may wait up to 30 seconds for Nucleus's
canonical account operation. An already processing job is recovered before
this check.

Dispatch atomically moves the lowest-sequence priority envelope, or the
lowest-sequence normal envelope when no priority job is queued, to
`processing/`. It changes the receipt to `processing`, increments its attempts
from zero to one, and starts its database source delivery. A priority arrival
never preempts a processing job, and a continuing priority stream can starve
the normal lane; there is no starvation protection.

A job receives no second processing attempt. Dispatch freezes current HEAD
and library instructions for each new examination. A fresh job that retains a
new work enters model-assisted integration with immediate application. A fresh job
whose exact bytes select an existing work completes with `duplicate` retention
and result `retained`, without an examination, reconciliation, or commit.

Content identity is resolved before the incoming filename is considered as a
label, so a duplicate keeps the retained work's canonical label even when its
basename is unusable or belongs to another work. Explicit manual `integrate`
remains available for deliberate integration of an already retained work.

Applied and recorded envelopes move whole from `processing/` to `done/`,
retained duplicate envelopes to `duplicates/`, failed envelopes to `failed/`,
and operator-skipped envelopes to `skipped/`. Every job-processing error fails
the source delivery and archives the job on its first attempt. A known
item-local source error lets the activation continue. An unexpected model,
runner, or runtime processing failure ends the activation nonzero after
archival; successors remain queued for the next activation. Historical
archives are not reclassified. There is no item or activation-lifetime limit,
and newly settled arrivals are registered between jobs.

## Pause and resume

`inbox pause` is an idempotent dispatch barrier. If a delivery is active, it is
allowed to finish, but no later queued job starts. A short-lived queue-control
lock orders pause against dispatch: if dispatch wins, that job is the current
job allowed to finish; once `pause` returns, no additional job can be claimed.
Registration remains available while paused, including the registration phase
of scheduled `inbox run` activations. Direct enqueue and queued-job priority
changes also remain available. Such an activation exits successfully after
registering arrivals, leaving the next envelope in `queued/`.

`inbox resume` idempotently removes only the operator pause. It does not start
a worker; dispatch resumes on the next external scheduler activation or an
explicit `inbox run`. The operator-owned `.paused` state is independent of the
Annals-owned `.maintenance` deployment boundary, and `resume` never removes
maintenance. It refuses to clear the pause while a retry event is preparing,
running, or halted, so ordinary dispatch cannot interleave with an unfinished
event. Maintenance blocks registration, direct enqueue, priority changes,
repair, retry execution, and ordinary dispatch.

## Interrupt one job

`inbox interrupt` durably requests that the named processing job stop and
requires an explicit `failed` or `skipped` disposition. `--reason` records
optional operator context. The job ID prevents a request from selecting a
later job if the observed job finishes first. An accepted request stops the
active liaison and archives the envelope in the selected directory.

It does not establish a pause, so the worker may continue with the next queued
job; run `inbox pause` first to keep later jobs queued. A skipped job receipt
has state `skipped`, but its already-started source delivery has status
`failed`, no result, and error code `inbox_job_skipped`.

Interruption returns a conflict as too late when the job already has a durable
terminal delivery outcome or an applied or recorded reconciliation. A pending
reconciliation remains interruptible until inbox automatic application begins.

## Source eligibility

Only visible top-level regular files not ending in `.part` are candidates for
automatic registration. Eligible files are registered in persisted first-seen
order into the normal lane. Dispatch prefers the priority lane and follows
immutable sequence within each lane. Invalid UTF-8, empty input, unusable
filename-derived labels, label conflicts, and other known item-local source
errors are archived as failed on the first attempt, and draining continues.

Unexpected model, runner, and runtime processing failures are also archived as
failed on the first attempt, but `inbox run` then exits nonzero and leaves
successors for the next activation. An arrival still settling at the final
rescan, or racing the final empty check, waits for the next activation.

## Retry failed deliveries

`inbox retry preview` is a read-only selection check. Both `--from` and
`--through` are required and must name terminal failed inbox jobs. Annals
orders failed source deliveries by `(completed_at, delivery ID)`, resolves
both anchors in that order, and selects the inclusive interval. This is
failure order. Priority dispatch can make it differ from job sequence.

The preview reports the ordered candidate jobs, delivery IDs, failure details,
and count without creating an event or a child job. A failed delivery already
used as an original in another event remains visible in its interval but is
marked ineligible with that prior event and any child provenance.

Reversed anchors and an anchor that is absent, not failed, or not an inbox
delivery are errors. The whole preview also fails if any selected delivery
lacks its matching terminal envelope, unchanged retained source identity, or
archived material.

Only failures after work retention are retryable: a pre-retention source error
has no durable digest against which Annals can validate its archive, so
correct the source and deliver it as a new job instead. There are no omitted,
open-ended, or retry-all bounds. An operator-skipped job is not a failed-job
candidate even though its source delivery has failed status.

The two anchors may be equal to select one failed job.

JSON preview output contains `from_job_id`, `through_job_id`, and ordered
`items`. Each item exposes its zero-based `ordinal`, original job, sequence,
delivery, completion time, and error. Nullable `already_selected_by`,
`already_selected_child_job_id`, and
`already_selected_child_delivery_id` carry prior retry provenance; null means
the item is eligible.

### Start the selected retry

`inbox retry start` resolves the same interval and freezes that exact ordered
membership in one durable event before processing it. The optional reason must
be trimmed, nonempty operator context of at most 1,000 characters; Annals
retains it with the event. Start requires the operator pause to be set, no
processing job, no other unfinished retry event, and no deployment
maintenance.

It rejects the complete window when any member is ineligible and never
silently drops a member. Ordinary arrivals remain intact and registrable while
the pause is set, but the retry runner's run lock excludes a simultaneous
scheduled activation and no ordinary queued job interleaves with the event.

For every frozen member, Annals preserves the original failed envelope and
failed delivery record and creates a fresh retry child job and source delivery
linked to both the event and original. The child envelope copies the original
unchanged source material; it never moves material out of `failed/`. Retry
children run sequentially in the frozen failure order and each has one
attempt.

They are event-controlled even if Annals uses a spool priority lane
internally; ordinary priority dispatch does not select or order them during
the event. Their explicit retry intent bypasses the fresh-job duplicate
cutoff: recognizing already retained bytes does not end the child with result
`retained`. Annals instead continues into integration.

It may finish or reuse the exact pending, applied, or recorded reconciliation
owned by the original failed attempt when its ownership and context still
validate; otherwise it begins a fresh examination. Retry does not blindly
force reexamination and never adopts an unrelated reconciliation for the same
work.

In particular, a pending record is reusable only while HEAD and the selected
instruction revision still equal its frozen basis. A stale or superseded
record is not handed to the child. Otherwise the child begins a fresh
examination under current instructions. Recorded or applied results remain
authoritative after instruction selection changes or later Nucleus failures.

Publication supports recovery across SQLite and the spool. The event is
`preparing` while Annals publishes its frozen items. Recovery creates or
recognizes each item's exact child without expanding selection or duplicating
an attempt. The event becomes `running` while children are processed. Before
the first zero-attempt child claim in each start or continue invocation,
Annals performs the same authenticated account preflight as ordinary dispatch.

A failed preflight changes the event to `halted` but leaves every remaining
child queued with attempts zero and creates no child delivery or model-run
row. The storage gate is also checked before every queued child: insufficient
space or a failed probe halts the event without starting that child.

After correcting the condition, use `inbox retry continue`; attended retry
events do not resume from the ordinary scheduler. A known item-local failure
terminalizes its child and advances to the next frozen item. An unexpected
model, runner, or runtime failure terminalizes the current child, changes the
event to `halted`, exits nonzero, and leaves later members `not_attempted`.

Interrupting an active retry child with either disposition also halts the
event after archiving that member; its outcome is `failed` or `skipped` as
requested. The outer pause is already set, so this is the operator stop
mechanism for the event.

### Continue an interrupted retry

`inbox retry continue EVENT_ID` requires the same paused, quiescent,
non-maintenance state as start. It completes interrupted publication when
needed, accepts a crash-stale `running` event after acquiring the run lock, and
advances only the selected event's `not_attempted` items. It never retries a
failed or skipped child. Continuing a completed event is a conflict; an unknown
event ID is not found. An event becomes `completed` only when all frozen items
are terminal. A later bounded event may select a failed child, making another
attempt an explicit chain; use the same child for both bounds when it is the
only desired member.

### Read retry outcomes

`inbox retry status EVENT_ID --details` reports the durable event bounds,
reason, state, lifecycle times, latest halt details, a summary, and ordered items. The
summary reports selected, attempted, succeeded, unsuccessful, and remaining
totals plus each outcome count. Each item pairs the original job, delivery,
and failure with its linked child job and delivery and derives one outcome:
`not_attempted`, `processing`, `applied`, `recorded`, `failed`, or `skipped`.

The aggregates are derived too, not copied counters, so the report remains
consistent with delivery history after recovery. A missing child or a queued
zero-attempt child is `not_attempted`. Without an event ID, `status` lists the
20 most recent completed events plus the one unfinished event, if present.
Neither form mutates the event or spool.

The default event status and start/continue receipts report identity, frozen
window, state, counts, remaining work, and latest halt. Add `--details` to
`retry status EVENT_ID` for the full original-to-child mapping. Human and JSON
modes select the same content.

The detailed JSON event report has `event`, `summary`, and `items`. `event` carries the
bounds, optional reason, lifecycle fields, optional `last_halt`, and member
count. `summary` carries the totals described above. Each item repeats its
frozen original snapshot, adds nullable child job, sequence, delivery,
lifecycle, result, revision, and error fields, and ends with its derived
`outcome`. The no-ID list form returns event records with `has_more`; it defaults to 20
completed events plus an unfinished event when present. Increase a positive
`--limit` to read more.

Start and continue exit zero when the event reaches `completed`, even when its
durable report contains item-local `failed` or `skipped` outcomes. They exit
nonzero when preflight, an unexpected processing error, or an operator
interruption leaves the event `halted`. The event report, not the process exit
code alone, is the success/failure accounting surface.

## Read inbox status

Human `inbox status` reports incoming files split into ready and settling, the
total queued count and its priority subset, processing envelopes, the next and
active jobs' identities and priorities, terminal archives including skipped
jobs, whether a worker is active, and the independent paused and maintenance
states. These are observed durable and lock facts, not inferred delivery
success from a process listing. Incomplete initialized state needs setup or
recovery; reads leave an absent spool absent.

JSON exposes the subset as `priority_queued` and each next or active job's
`priority`; `attempts`, `started_at`, and `interrupt_requested` remain
specific to `active_job`. It also reports ignored entries. Both forms include
the live storage gate. JSON `storage` contains `enabled`,
`minimum_available_bytes`, `ready`, and library/inbox `locations` with their
measured `available_bytes`.

Human `inbox run` reports registered, attempted, applied, recorded,
duplicates, failed, skipped, remaining, settling, whether the runnable queue
was drained, and whether pause, maintenance, or low storage stopped dispatch.
`queue_drained` is false whenever `queued/` or `processing/` is nonempty,
including a healthy paused or storage-gated queue.

JSON uses `duplicates` and `skipped` for their archive counts and adds
`stopped_for_low_space` plus the most recent `storage` check when a queued
claim was considered. It also includes the spool root, effective settling
interval, elapsed time, recovery count, and ignored count. The external
Clockwork or systemd schedule remains the wake-up and recovery mechanism;
Annals has no resident daemon or internal scheduler.

Human low-space deferral emits one diagnostic even under `--quiet`; JSON keeps
that condition in its success document without writing a success diagnostic to
standard error. See [installation](installation.md) for configuration and
scheduling.

Because registration and direct enqueue do not start a source delivery, queued
jobs appear in `inbox status` but not in `lately`. They enter source-delivery
history when dispatched.


## Spool records

The Annals library retains corpus state and history. The spool is a visible
delivery queue with a small Annals-owned ordering index:

```text
.queue.json
.run.lock
.control.lock
.paused             # operator-owned dispatch state
.maintenance        # deployer-owned maintenance state
incoming/
`-- report.md
queued/
`-- JOB_ID/
    |-- job.json
    `-- material/
        `-- report.md
processing/
`-- JOB_ID/
    |-- job.json
    |-- interrupt.json # present only after an operator request
    `-- material/
        `-- report.md
done/JOB_ID/       # the completed envelope
duplicates/JOB_ID/ # a fresh duplicate completed at retention
failed/JOB_ID/     # the permanently failed envelope
skipped/JOB_ID/    # the operator-skipped envelope
```

Annals moves the file into a unique job envelope on the same filesystem. It
does not rewrite its contents or basename. Moving the envelope to `done`,
`duplicates`, `failed`, or `skipped` prevents archive collisions without
changing `report.md`. The `material` subdirectory means even a source named
`job.json` or `interrupt.json` cannot collide with operational state. The
same-filesystem moves preserve the source's bytes, basename, inode, mode, and
modification time.

The current `job.json` receipt format is version 6 and is authoritative after
registration, direct enqueue, or retry publication. `.queue.json` assigns a
UTC first-seen time and prospective monotonic sequence when Annals first
observes incoming material; pathname bytes break observation ties.

Registration moves the source into `queued/` and preserves the sequence in a
receipt with state `queued`, `priority` set to `normal`, attempts zero, and no
database source-delivery record. Direct enqueue instead copies an explicitly
selected source into a complete queued envelope, leaves the original
unchanged, and can set `priority` to `priority`.

Dispatch moves the lowest-sequence priority job, or the lowest-sequence normal
job when no priority job is queued, to `processing`, changes its receipt state
to `processing`, increments its attempts to one, and starts the delivery
record. A job receives no second processing attempt.

Ordinary version-6 receipts have null retry provenance. A retry child's
receipt sets `retry_event_id`, `retry_ordinal`, `retry_of_job_id`, and
`retry_of_ingestion_id` together. It may also set `retry_reconciliation_id`
when the original attempt owns one exact reconciliation eligible for
validation and reuse. `retry_ordinal` is the zero-based position in the
event's frozen failure order.

The first four fields are all present or all absent; a reconciliation ID is
never a license to adopt unrelated work history. Existing version-5 receipts
are accepted with null retry fields; a normal later receipt rewrite emits
version 6 while keeping job identity, attempt, priority, source-delivery
linkage, and terminal archive. Selecting a failed original for retry does not
itself rewrite that historical receipt.

`.run.lock` prevents overlapping ordinary or retry workers. `.control.lock` is
held only around registration, direct enqueue, queued-job priority changes,
ordinary or retry dispatch, retry-child publication, sequence allocation,
pause changes, interruption, and terminal disposition; it is never held during
liaison work. `.paused` is the operator-owned dispatch gate managed by `annals
inbox pause` and `resume`. A validated `interrupt.json` is a durable request
bound to its named processing job.

The Annals deployer temporarily creates `.maintenance` to make a running worker
stop cleanly after its current job and to prevent other spool mutation during
cutover. These operational files are not retained works. Do not create,
remove, or edit their contents directly.

The inbox storage gate is separate from those marker files. Before a queued
claim it requires `minimum_available_bytes` on both the library and spool
filesystems. It is derived from current filesystem state, creates no marker,
and therefore needs no `resume` when storage recovers.


## Crash recovery

Recovery never invokes a second liaison for a receipt whose attempts value is
already positive. It first finishes durable success from that attempt when
possible: for example, it can archive a conclusively retained duplicate or
finish the exact reconciliation linked through the job receipt and model-run
token. It does not adopt an unrelated reconciliation for the same work.

If no durable success exists, recovery fails the interrupted delivery and
archives the job. A durable interrupt request selects the requested failed or
skipped archive; a skipped job's source delivery is failed with
`inbox_job_skipped`. Startup also removes an empty envelope left before a
claim move and reconstructs a missing receipt when its envelope already
contains exactly one moved material file.

If a worker is killed during examination, recovery retires only the model-run
token owned by that receipt and marks its open reconciliation draft abandoned;
it does not close a separate manual examination of the same work.

Spool recovery also performs the queue-state migration from releases that had
no `queued/` directory. A legacy `processing/` envelope with zero attempts is
moved to `queued/` and receives the queued receipt state and immutable
sequence; an envelope with an attempt or other processing progress is
recovered without another liaison and then completed or failed according to
its durable state.

Recovery raises the sequence allocator above every migrated and archived job
so later registration cannot overtake existing material within its lane.
Receipt migration assigns historical jobs normal priority. This is a
filesystem migration only and requires no SQLite schema change. It is
idempotent if an activation is interrupted and repeated.

A SQLite commit and the following receipt and directory update cannot be one
atomic transaction. Recovery may therefore repeat idempotent retention or
terminal archival work in that narrow crash interval. The work's
content-addressed storage keeps the exact source bytes stable, and durable job
progress lets recovery finish success without starting another liaison. Once
an attempt is recorded, an interrupted job is never examined again.

Retry publication crosses the same SQLite-and-spool boundary. The complete
membership is durable before processing, and an event remains `preparing`
until every exact child is published. `retry continue` recovers an interrupted
publication idempotently: it recognizes a child already present or publishes
the one missing child, without widening the event or duplicating an attempt.

## Scheduled failure policy

Both `annals/inbox` and `annals/decisions-inbox` use Clockwork definition schema
2 with `[failure] on_abend = "halt-until-approved"`. The release-local runner
selects `inbox run --stop-on-failure`. This batch option stops after its first
failed job, including an item-local source failure, and returns nonzero before
claiming a successor. It does not create an Annals scheduling-pause record.
Ordinary manual `inbox run` retains its item-local continuation behavior.

Clockwork retains a pending failure episode and permits later scheduled
activations before the shared service-health threshold. By default, five
consecutive failed read-only checks, at least 60 seconds apart, halt the binding
and make its email alert eligible together. A healthy check or inactive intent,
including operator pause, clears the pending episode. Checks do not dispatch
or retry sources. Inspect `clockwork incident list
annals/inbox` or the `annals/decisions-inbox` key and `clockwork incident show
INCIDENT_ID`. Only explicit approval followed by `clockwork binding resume KEY
INCIDENT_ID` releases that scheduling halt. Definition switches, deployment,
Annals `inbox resume`, and dependency recovery do not release it.

A low-storage readiness result, operator pause, maintenance, or empty queue is
not an abend. A storage-probe or authentication error is an abend. Annals retains
operator pauses, bounded retry-event halts, exact attempts, and domain recovery.
Scheduling continuation neither retries a failed delivery nor clears these
product controls. Existing failed archives are history, not new incidents. A runtime failure after
a recorded reconciliation preserves the completed delivery and result, then
stops the scheduled batch before its successor.

## Privacy, authority, and limits

Spool archives retain unchanged source text. Libraries, receipts, logs, and
Nucleus state can retain complete source material, instructions, model context,
and diagnostics. Keep them private. A Nucleus restart marks unfinished attempts
lost; Annals decides subsequent domain recovery from its own durable records.
Nucleus runtime failure does not erase an already recorded reconciliation or
committed result. [Interpretation](work-integrate.md) owns model preparation,
exact-context reuse, Bazaar selection, and scoped requester-tool guarantees.

The default reserve is 7,000,000,000 bytes and `0` disables the gate. Changing
or disabling it as low-storage remediation requires exact target-scoped user
consent. Settling defaults to 60 seconds and can be zero. Priority has two
lanes and no fairness protection. Each job has at most one attempt. Retry
membership is bounded and immutable. There is no queue-capacity, maximum-file-size,
throughput, drain-deadline, scheduling-interval, completion-latency, or storage
restoration guarantee. Response bounds do not bound replay cost.

Feature contract, provider release, library schema, receipt shape, Nucleus
protocol, and Clockwork definition schema are distinct compatibility identities.
Supported migration and recovery preserve historical envelopes. No general
cross-release spool support or deprecation window is promised. Direct receipt
editing, terminal-envelope requeue, direct SQL, open-ended retry, and a second
attempt for an existing job are unsupported.

CLI usage recording requires nonempty `CODEX_THREAD_ID`. Chancery records
command identity, time, and thread ID, not arguments, output, or outcomes.
Internal calls are excluded. Recording errors preserve command results.

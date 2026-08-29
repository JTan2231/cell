# CLI contract

## Global options

```text
annals [--config PATH] [--library PATH] [--json] [--quiet] [-v...] COMMAND
```

The config path resolves from `--config`, then a nonempty `ANNALS_CONFIG`.
The library path resolves from `--library`, then a nonempty `ANNALS_LIBRARY`,
then the selected config's `library`. If neither a library nor a usable config
selects one, the command fails with `library_not_configured`; Annals never
falls back to `./annals.db`.

The installed macOS frontend selects
`$HOME/Library/Application Support/Annals/config.toml` only when the invocation
has no explicit config or library selection. Thus bare installed commands such
as `annals stats` use the user installation. Explicit selections such as these
target independent libraries. The literal `annals` invocation with no
subcommand still displays help.

```text
annals --config ./project.toml stats
annals --library ./scratch.db init
ANNALS_CONFIG=./project.toml annals stats
ANNALS_LIBRARY=./scratch.db annals stats
```

An explicit library suppresses the frontend's user-config default. The
uninstalled executable has no implicit config path, so repository and Linux
uses must provide a config or library unless their own launcher supplies one.
Relative `library` and `inbox.root` config paths are resolved from the config
file's directory; command-line and environment paths remain relative to the
process working directory.
`--json` emits one success object on stdout or one error object on stderr.
`--quiet` suppresses successful human mutation messages. `-v` prints the
resolved library path on stderr in human mode.

## Library operations

```text
annals init
annals migrate
annals stats
annals validate
annals backup OUTPUT
```

`init` creates revision zero and refuses to replace an existing library.
`migrate` upgrades a version-3 library to version 4 by adding bounded inbox
retry provenance; it does not reinterpret works, deliveries, reconciliations,
or corpus history. Version 3 remains the deliberate fresh-state boundary, so
the command rejects libraries older than version 3 without mutating them and
refuses libraries created by a newer executable. Repeating `migrate` on a
version-4 library is an idempotent current-format check. Use the macOS
deployer's guarded `--fresh-state` cutover when replacing a pre-version-3
installed library. The version-3-to-4 migration is one transaction; failure
leaves the library at version 3 without partial retry tables.
`stats` reports revision and corpus, graph, work, reconciliation, history,
model-run, and database-size information.

`validate` checks SQLite, foreign keys, the storage boundary, retained-work and
audit-artifact hashes, normalized request and draft lifecycle, contiguous
typed effects, every replayed corpus state, and change, shake, revert, and
reconciliation provenance. It does not repair state.

`backup` makes a consistent SQLite copy and refuses to replace its destination.

## Immutable works

```text
annals work add INPUT [--name LABEL]
annals work list
annals work show LABEL
```

`INPUT` is a UTF-8 file containing non-whitespace source text, or `-`. A file
defaults to its UTF-8 filename stem; stdin requires `--name`. Work labels are
nonempty and normalized-unique. Exact retained bytes are content-addressed by
SHA-256. Supplying them again, even with another requested label, selects the
original work and label. A label already attached to different bytes is a
conflict.

Adding a work does not change the corpus revision. Human `work list` shows
labels and sizes; JSON also reports SHA-256 digests and `first_retained_at`.
Human `work show` labels that timestamp `First retained`. It is the time Annals
first retained those content-addressed bytes, not the source file's creation or
modification time. `work show` also reports Markdown heading paths and the
complete unchanged text. Source heading paths describe the document; they are
not concept paths.

## Model-assisted integration

```text
annals integrate INPUT [--name LABEL] [--quality QUALITY] [--model MODEL] [--apply] [--reexamine]
annals integrate --work LABEL [--quality QUALITY] [--model MODEL] [--apply] [--reexamine]
```

The first form retains or recognizes and examines the selected work. The second
examines an already retained work. Both are explicit manual integration and
retain this behavior when the bytes were supplied before. Annals freezes the
current corpus revision, invokes the liaison, and expects one recorded
reconciliation. The liaison starts a complete draft with
`submit_reconciliation`. If Annals reports `needs_changes`, independently valid
operations remain staged while `revise_reconciliation` changes only named
operation IDs. `reconciliation_status` recalls the compact roster or exact
stored operations, and `discard_reconciliation` abandons the complete draft so
a fresh submission can start. Submission or revision records automatically
when every active operation works together. The model's final response is
diagnostic and is not parsed as the reconciliation.

Annals may reuse the newest successful reconciliation for the exact same work,
base revision, prompt version, model, and reasoning effort. `--reexamine`
bypasses this lookup. A later corpus revision or changed liaison configuration
starts a fresh examination.

`--quality` accepts three presets. Its value resolves from the command line,
then `[liaison].quality` in the selected config, then `high`:

| Quality | Model | Reasoning effort |
| --- | --- | --- |
| `low` | `gpt-5.6-luna` | `medium` |
| `medium` | `gpt-5.6-terra` | `medium` |
| `high` | `gpt-5.6-sol` | `max` |

`--model` resolves from the command line, then `[liaison].model`, then the
model selected by the quality preset. It changes only the model; the selected
quality continues to choose reasoning effort. `[liaison].nucleus_socket`
optionally selects a nonstandard Nucleus Unix socket. Annals submits the same
prompt, base and developer instructions, model, reasoning effort, and exact
nine-tool contract as one Nucleus job. There is no direct Codex fallback.
Nucleus owns the isolated app-server process, persistent authentication,
credential refresh, and serialization. Annals asks Nucleus for an
authenticated account preflight before a queued dispatch; it may wait up to 30
seconds, and failure leaves the envelope queued with attempts zero.

The liaison submits a provisional, best-current interpretation. It does not
filter source material by estimated novelty or salience and does not claim an
objective final decomposition into atomic concepts.

Without `--apply`, a reconciliation whose projected corpus state differs from
its base remains pending. `--apply` immediately commits that pending
transition. A projected corpus state mechanically equal to the base is stored
with status `recorded`; it creates no commit and does not advance the revision.
Optional annotations are inert and never block application.

## Consumption telemetry

The separate companion CLI has three reporting commands:

```text
annals-usage report [--json] [--limit N] [--config PATH]
annals-usage budget [--json] [--config PATH]
annals-usage doctor [--config PATH]
annals-usage login --device-auth
```

`report` reads current Annals jobs and ordered model output from Nucleus and
attributes their observed model-run attempts to recent source deliveries. Its
coverage field distinguishes exact per-response totals,
cumulative fallbacks, known zero-use deliveries, pending work, reused
examinations, and output gaps. `budget` displays a live,
account-global Codex allowance snapshot and labels the account's lifetime and
daily token activity as contextual rather than allowance units. The backend
exposes neither a token denominator for that allowance nor a per-delivery
subscription share. Neither command retains a reporting database or account
snapshot. `doctor` checks the companion configuration, the Annals paths,
Nucleus, and authenticated account-telemetry access. `budget`
and `doctor` report that authentication is busy instead of waiting when another
Nucleus operation owns the credential lease. `login` delegates to `nucleus auth
login`, so Annals never owns credential files.

The token categories overlap: cached and cache-write tokens are subsets of
input, reasoning tokens are a subset of output, and total is input plus output.
The complete accounting and configuration contract is documented in
[Consumption telemetry](telemetry.md).

## Scheduled inbox

```text
annals inbox register [--settle-seconds SECONDS]
annals inbox enqueue [--priority] FILE...
annals inbox prioritize JOB_ID...
annals inbox deprioritize JOB_ID...
annals inbox run [--settle-seconds SECONDS]
annals inbox pause
annals inbox resume
annals inbox interrupt JOB_ID --as failed|skipped [--reason TEXT]
annals inbox retry preview --from JOB_ID --through JOB_ID
annals inbox retry start --from JOB_ID --through JOB_ID [--reason TEXT]
annals inbox retry status [EVENT_ID]
annals inbox retry continue EVENT_ID
annals inbox status
```

All commands except `inbox retry status` require an `[inbox]` config section
with `root`; retry-event reports are durable library reads and can be selected
with `--library` alone. The optional config key `settle_seconds` defaults to 60;
the `register` and `run` flags override it. A zero settling interval is allowed.
`inbox status`, `inbox retry preview`, and `inbox retry status` are read-only.

`inbox register` moves every settled file into a durable queued job without
processing it. Each file moves, without changing its basename or bytes, into
`queued/JOB_ID/material/` beside an operational `job.json` receipt. The
receipt has state `queued`, attempts zero, and an immutable monotonic sequence.
Registration creates no database source-delivery record. Human output reports
the registered jobs; JSON includes each assigned job ID and sequence with
`priority` set to `normal`.

`inbox enqueue` copies each explicitly named regular file directly into a new
durable queued envelope and leaves the original file unchanged. It bypasses
`incoming/` and the settling interval: the envelope becomes dispatchable only
after its material and receipt are complete, so admission cannot race a
partial copy. Files receive immutable monotonic sequences in argument order
and enter the normal lane unless `--priority` selects the priority lane. The
result reports the spool root, selected priority, registered count, each job's
ID, sequence, and priority, the total queued and priority-queued counts, and
the next job. Like registration, enqueue starts no source delivery.

`inbox prioritize JOB_ID...` moves the named queued jobs to the priority lane;
`inbox deprioritize JOB_ID...` moves them to the normal lane. Both operate only
on jobs that are still under `queued/`, hold the queue-control lock for the
mutation, and leave each job ID and immutable sequence unchanged. Argument
order therefore does not reorder jobs; an older normal job moved to priority
can precede newer priority jobs. Requesting the lane a job already has is an
idempotent success. The result reports the spool root, requested and changed
counts, selected priority, requested jobs, the priority-queued count, and the
next job. Naming a processing or terminal job, or an unknown job ID, is an
error rather than a request to alter history. A retry child is controlled by
its retry event and cannot be prioritized or deprioritized independently.

`inbox run` takes the activation-long spool lock, performs the same
registration phase, and drains jobs sequentially while processing is allowed.
After recovery and registration, it performs one authenticated account
preflight before its first queued dispatch. The preflight does not claim a job,
increment attempts, or start a source delivery. If it fails, `inbox run` exits
nonzero while the next envelope remains under `queued/` with attempts zero and
no database delivery record. An already processing job is recovered before
this check.

Dispatch atomically moves the lowest-sequence priority envelope, or the
lowest-sequence normal envelope when no priority job is queued, to
`processing/`. It changes the receipt to `processing`, increments its attempts
from zero to one, and starts its database source delivery. A priority arrival
never preempts a processing job, and a continuing priority stream can starve
the normal lane; there is no starvation protection. A job receives no second
processing attempt. A fresh job that retains a new work enters model-assisted
integration with immediate application. A fresh job whose exact bytes select an
existing work completes with `duplicate` retention and result `retained`,
without an examination,
reconciliation, or commit. Content identity is resolved before the incoming
filename is considered as a label, so a duplicate keeps the retained work's
canonical label even when its basename is unusable or belongs to another work.
Explicit manual `integrate` remains available for deliberate integration of an
already retained work.

Applied and recorded envelopes move whole from `processing/` to `done/`,
retained duplicate envelopes to `duplicates/`, failed envelopes to `failed/`,
and operator-skipped envelopes to `skipped/`. Every job-processing error fails
the source delivery and archives the job on its first attempt. A known
item-local source error lets the activation continue. An unexpected model,
runner, or runtime processing failure ends the activation nonzero after
archival; successors remain queued for the next activation. Historical
archives are not reclassified. There is no item or activation-lifetime limit,
and newly settled arrivals are registered between jobs.

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

`inbox interrupt` durably requests that the named processing job stop and
requires an explicit `failed` or `skipped` disposition. `--reason` records
optional operator context. The job ID prevents a request from selecting a
later job if the observed job finishes first. An accepted request stops the
active liaison and archives the envelope in the selected directory. It does
not establish a pause, so the worker may continue with the next queued job;
run `inbox pause` first to keep later jobs queued. A skipped job receipt has
state `skipped`, but its already-started source delivery has status `failed`,
no result, and error code `inbox_job_skipped`. Interruption returns a conflict
as too late when the job already has a durable terminal delivery outcome or an
applied or recorded reconciliation. A pending reconciliation remains
interruptible until inbox automatic application begins.

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

Recovery never starts a second liaison when a processing receipt already has
an attempt. It may finish durable success left by that attempt, such as a
conclusively retained duplicate or the job's exact linked reconciliation. If
there is no durable success to finish, it fails and archives the interrupted
job. A durable interrupt request preserves its selected failed or skipped
disposition through recovery.

### Bounded retry events

`inbox retry preview` is a read-only selection check. Both `--from` and
`--through` are required and must name terminal failed inbox jobs. Annals orders
failed source deliveries by `(completed_at, delivery ID)`, resolves both
anchors in that order, and selects the inclusive interval. This is failure
order, not job sequence: priority dispatch can make those orders differ. The
preview reports the ordered candidate jobs, delivery IDs, failure details, and
count without creating an event or a child job. A failed delivery already used
as an original in another event remains visible in its interval but is marked
ineligible with that prior event and any child provenance. Reversed anchors and
an anchor that is absent, not failed, or not an inbox delivery are errors. The
whole preview also fails if any selected delivery lacks its matching terminal
envelope, unchanged retained source identity, or archived material. Only
failures after work retention are retryable: a pre-retention source error has
no durable digest against which Annals can validate its archive, so correct the
source and deliver it as a new job instead. There are no omitted, open-ended,
or retry-all bounds. An operator-skipped job is not a failed-job candidate even
though its source delivery has failed status. The two anchors may be equal to
select one failed job.

JSON preview output contains `from_job_id`, `through_job_id`, and ordered
`items`. Each item exposes its zero-based `ordinal`, original job, sequence,
delivery, completion time, and error. Nullable `already_selected_by`,
`already_selected_child_job_id`, and
`already_selected_child_delivery_id` carry prior retry provenance; null means
the item is eligible.

`inbox retry start` resolves the same interval and freezes that exact ordered
membership in one durable event before processing it. The optional reason must
be trimmed, nonempty operator context of at most 1,000 characters; Annals
retains it with the event. Start requires the operator pause to be set, no
processing job, no other unfinished retry event, and no deployment maintenance.
It rejects the complete window when any member is ineligible and never silently
drops a member.
Ordinary arrivals remain intact and registrable while the pause is set, but the
retry runner's run lock excludes a simultaneous scheduled activation and no
ordinary queued job interleaves with the event.

For every frozen member, Annals preserves the original failed envelope and
failed delivery record and creates a fresh retry child job and source delivery
linked to both the event and original. The child envelope copies the original
unchanged source material; it never moves material out of `failed/`. Retry
children run sequentially in the frozen failure order and each has one attempt.
They are event-controlled even if Annals uses a spool priority lane internally;
ordinary priority dispatch does not select or order them during the event.
Their explicit retry intent bypasses the fresh-job duplicate cutoff:
recognizing already retained bytes does not end the child with result
`retained`. Annals instead continues into integration. It may finish or reuse
the exact pending, applied, or recorded reconciliation owned by the original
failed attempt when its ownership and context still validate; otherwise it
begins a fresh examination. Retry does not blindly force reexamination and
never adopts an unrelated reconciliation for the same work. In particular, a
pending record is reusable only while HEAD still equals its base; a stale or
superseded record is not handed to the child.

Publication is recoverable across the SQLite-and-spool boundary. An event is
visible as `preparing` while its durable frozen items are being published, and
recovery creates or recognizes each one exact child without widening the
selection or duplicating an attempt. It becomes `running` while children are
processed. Before the first zero-attempt child claim in each start or continue
invocation, Annals performs the same authenticated account preflight as
ordinary dispatch. A failed preflight changes the event to `halted` but leaves
every remaining child queued with attempts zero and creates no child delivery
or model-run row. A known item-local failure terminalizes its child and
advances to the next frozen item. An unexpected model, runner, or runtime
failure terminalizes the current child, changes the event to `halted`, exits
nonzero, and leaves later members `not_attempted`. Interrupting an active retry
child with either disposition also halts the event after archiving that member;
its outcome is `failed` or `skipped` as requested. The outer pause is already
set, so this is the operator stop mechanism for the event.

`inbox retry continue EVENT_ID` requires the same paused, quiescent,
non-maintenance state as start. It completes interrupted publication when
needed, accepts a crash-stale `running` event after acquiring the run lock, and
advances only the selected event's `not_attempted` items. It never retries a
failed or skipped child. Continuing a completed event is a conflict; an unknown
event ID is not found. An event becomes `completed` only when all frozen items
are terminal. A later bounded event may select a failed child, making another
attempt an explicit chain; use the same child for both bounds when it is the
only desired member.

`inbox retry status EVENT_ID` reports the durable event bounds, reason, state,
lifecycle times, latest halt details, a summary, and ordered items. The summary
reports selected, attempted, succeeded, unsuccessful, and remaining totals plus
each outcome count. Each item pairs the original job, delivery, and failure
with its linked child job and delivery and derives one outcome:
`not_attempted`, `processing`, `applied`, `recorded`, `failed`, or `skipped`.
The aggregates are derived too, not copied counters, so the report remains
consistent with delivery history after recovery. A missing child or a queued
zero-attempt child is `not_attempted`. Without an event ID, `status` lists the
20 most recent completed events plus the one unfinished event, if present.
Neither form mutates the event or spool.

The JSON event report has `event`, `summary`, and `items`. `event` carries the
bounds, optional reason, lifecycle fields, optional `last_halt`, and member
count. `summary` carries the totals described above. Each item repeats its
frozen original snapshot, adds nullable child job, sequence, delivery,
lifecycle, result, revision, and error fields, and ends with its derived
`outcome`. The no-ID list form returns an `events` array of event records.

Start and continue exit zero when the event reaches `completed`, even when its
durable report contains item-local `failed` or `skipped` outcomes. They exit
nonzero when preflight, an unexpected processing error, or an operator
interruption leaves the event `halted`. The event report, not the process exit
code alone, is the success/failure accounting surface.

Human `inbox status` reports incoming files split into ready and settling,
the total queued count and its priority subset, processing envelopes, the next
and active jobs' identities and priorities, terminal archives including
skipped jobs, whether a worker is active, and the independent paused and
maintenance states. JSON exposes the subset as `priority_queued` and each next
or active job's `priority`; `attempts`, `started_at`, and
`interrupt_requested` remain specific to `active_job`. It also reports ignored
entries.
Human `inbox run` reports registered, attempted, applied, recorded, duplicates,
failed, skipped, remaining, settling, whether the runnable queue was drained,
and whether pause or maintenance stopped dispatch. `queue_drained` is false
whenever `queued/` or `processing/` is nonempty, including a healthy paused
queue. JSON uses `duplicates` and `skipped` for their archive counts and adds
the spool root, effective settling interval, elapsed time, recovery count, and
ignored count. The external launchd or systemd schedule remains the wake-up
and recovery mechanism; Annals has no resident daemon or internal scheduler.
See the [system installation guide](system-installation.md) for the complete
spool, recovery, control, and scheduler contract.

Because registration and direct enqueue do not start a source delivery, queued
jobs appear in `inbox status` but not in `lately`. They enter source-delivery
history when dispatched.

## Recent source activity

```text
annals lately [--since TIME] [--until TIME]
              [--by created|modified|first-seen|ingested|completed]
              [--status processing|completed|failed]
              [--channel manual|inbox]
```

`lately` reports source-delivery metadata, independently of the source's text
or interpreted concepts. It never searches source content or emits source
text, headings, quotations, topics, or dates mentioned inside a work.

The report uses a UTC half-open interval: `since` is inclusive and `until` is
exclusive. `--until` defaults to the instant at which the report begins.
`--since` defaults to `7d`. A relative `--since` is subtracted from the resolved
`until`, so an explicit end and relative start produce a reproducible window.
Relative durations are a positive integer followed by `s`, `m`, `h`, `d`, or
`w`. Absolute values are either an RFC 3339 timestamp or a `YYYY-MM-DD` UTC
date, interpreted as midnight at the start of that date. RFC 3339 offsets are
accepted and normalized to UTC in output. The start must precede the end.

Examples:

```text
annals lately
annals lately --since 24h
annals lately --since 2026-08-01 --until 2026-08-15
annals lately --since 7d --by modified
annals lately --since 30d --status failed --by completed
annals lately --since 7d --channel inbox
```

`--by` chooses the timestamp used for both inclusion and newest-first ordering:

| Basis | Meaning |
| --- | --- |
| `created` | Filesystem creation time captured when the source arrived, when supplied by the operating system. |
| `modified` | Filesystem modification time captured when the source arrived. |
| `first-seen` | When Annals first observed the delivery. |
| `ingested` | When Annals retained the bytes as a new work or recognized them as an existing work. This is the default. |
| `completed` | When delivery processing reached the completed or failed state. |

Created and modified times are captured source metadata. They do not represent
authorship, publication, events described by the source, or continued watching
of the original path. Standard input has neither filesystem timestamp. A
failure before work retention has no ingestion time, and an active delivery
has no completion time.

`--status` accepts `processing`, `completed`, or `failed`. `--channel` accepts
`manual` or `inbox`. The manual channel covers a source passed to `work add` or
the input form of `integrate`; `integrate --work LABEL` selects an existing work
and is not another delivery. Filters are applied before the time window.

Every delivery has its own receipt. Delivering identical bytes again creates a
second receipt linked to the original immutable work. Retention is therefore
reported independently as `new` or `duplicate`. Lifecycle status is also
independent of the terminal result:

| Result | Meaning |
| --- | --- |
| `retained` | `work add` or a fresh duplicate inbox delivery completed at the retention boundary. |
| `pending` | Integration completed with a reconciliation awaiting application. |
| `applied` | Integration completed and created the reported corpus revision. |
| `recorded` | Integration completed without a corpus change. |

A retry child likewise appears as a new inbox source delivery, while its
original remains failed at its original completion time. `lately` reports each
delivery independently; use `inbox retry status` for their event and
parent-child relationship.

A processing delivery has not reached a terminal outcome and has no result.
An inbox job-processing error fails the delivery on its first attempt.
Source-bearing manual commands are serialized per library; the next such
command finalizes any receipt abandoned by an interrupted predecessor with
error `manual_ingestion_interrupted`. A failed delivery has status `failed`,
no result, and a structured error. It can still identify a work and retention
disposition when failure occurred after ingestion. An operator-skipped inbox
job is reported here as a failed delivery with error `inbox_job_skipped`. Work
retention and a `work add` completion are atomic, as are an input integration's
applied result and its corpus revision.

When the selected basis is unavailable, the delivery cannot be placed in the
window and is omitted. `missing_time_count` counts all such receipts matching
the status and channel filters. Human output states how many were omitted. To
inspect an early failure with no ingestion time, select `--by first-seen` or
`--by completed`.

Human output echoes the exact resolved range, time basis, and active filters;
reports lifecycle and retention counts; and lists the selected timestamp,
channel, status, source name, result, applied revision when present, and
retention disposition. Empty windows say `No source activity`.

JSON echoes `since`, `until`, `time_basis`, and the optional `status` and
`channel` filters. It includes delivery, lifecycle, retention, and missing-time
counts plus a `deliveries` array. Each delivery reports `source_name`,
`channel`, `status`, optional `retention` and `result`, optional work label,
optional source byte size and SHA-256, captured `source_created_at` and
`source_modified_at`, `first_seen_at`, optional `ingested_at` and
`completed_at`, optional `applied_revision`, and an optional structured
`error`. Error messages are reporting-safe lifecycle summaries; raw runner
diagnostics are never selected by this report. It exposes no storage-row
identifier or dedicated source-path field.

## Reconciliations and corpus changes

```text
annals change submit INPUT --work LABEL --base REVISION
annals change list
annals change show [--work LABEL | --at REVISION]
annals change validate [--work LABEL]
annals change apply [--work LABEL]
```

`change submit` reads strict reconciliation JSON from a file or `-`. The flags
provide the immutable evidence work and frozen corpus revision; both are
deliberately absent from the semantic request.

Submission resolves and validates the complete projected corpus state but does
not mutate the corpus. A result based on the same or a later revision
supersedes that work's previous pending reconciliation. An older-base result is
retained without displacing a newer pending result. `change list` includes
pending, applied, superseded, and recorded reconciliations.

With `--work`, `change show` selects that work's pending reconciliation when
one exists, otherwise its newest record. Without `--work`, it selects the sole
pending result; when none is pending, it succeeds only if exactly one work has
recorded results. `change validate` and `change apply` select pending results
only and require `--work` when more than one exists.

`change show --at REVISION` retrieves the commit at that revision. Its
`effects` are the exact material transition from the preceding revision to the
selected revision, using the same semantic entries as `diff PARENT REVISION`.
For an applied reconciliation it also shows the original graph-native request
and resolved operations. For a revert it shows the target revision and
resolved inverse. For a shake it shows the transitive-reduction request and
removed parent edges. All include the actor and timestamp.

Human reconciliation output renders public `cN` IDs alongside labels, local
creation handles, parent-edge changes, exact evidence quotations and source
context, evidence dispositions, replacements, and annotations. `change
validate` re-resolves and renders the same semantic facts without writing.
Resolved evidence reports one item per submitted selector together with its
`occurrence_count`; it never exposes the resolved ranges. Resolved operations
record what the request addressed, while `effects` report what actually
changed. An idempotent ensure may therefore appear in `resolved_operations`
without a matching effect.

`change apply` additionally requires HEAD to equal the base revision. Success
updates concepts, edges, evidence, reconciliation status,
history, and revision in one transaction.

### Reconciliation contract

A reconciliation contains a summary, one or more operations, and optional
free-form annotations:

```json
{
  "summary": "Integrate predicate locking and phantom prevention",
  "operations": [
    {
      "action": "add_evidence",
      "concept": {"id": "c12"},
      "evidence": [
        {
          "quote": "A serializable execution has the same effect as some serial execution."
        }
      ]
    },
    {
      "action": "create_concept",
      "ref": "predicate_locking",
      "label": "Predicate locking",
      "parents": [{"id": "c12"}, {"id": "c27"}],
      "evidence": [
        {
          "quote": "Predicate locks prevent inserts that would change the result of a previously evaluated predicate.",
          "within_heading": ["Transactions", "Avoiding phantom reads"]
        }
      ]
    },
    {
      "action": "add_parent",
      "concept": {"id": "c31"},
      "parent": {"new": "predicate_locking"}
    }
  ],
  "annotations": [
    "The work presents predicate locking as a phantom-prevention technique."
  ]
}
```

Every object rejects unknown fields. Summaries, annotations, labels, handles,
and quotations must be nonempty when present. Labels and handles have no outer
whitespace or control characters. `annotations` may be omitted and defaults to
an empty list. Annotations are retained as meta-level context only; they are
not evidence, confidence levels, or review flags and do not affect projected
corpus state, corpus validation, or application.

### Concept selectors

An existing concept is addressed by its durable public ID:

```json
{"id":"c42"}
```

Public IDs have a lowercase `c` followed by a positive canonical decimal
integer. They preserve identity across rewording and relationship changes.

A concept created in the same request declares a request-unique `ref` and is
selected by that handle:

```json
{"new":"predicate_locking"}
```

Local handles may be referenced anywhere in the request, including before the
corresponding creation appears. They are not labels. Different concepts may
have identical labels, so labels never select a concept.

There are no concept-path selectors. The only path arrays in the public
contract locate headings within source works.

### Evidence

Evidence always belongs to the work supplied by the host:

```json
{
  "quote": "Exact source language",
  "within_heading": ["Optional", "exact Markdown heading path"],
  "preceded_by": "Optional exact neighboring text",
  "followed_by": "Optional exact neighboring text"
}
```

`quote` is required. The other fields filter its occurrences by heading and
exact immediately adjacent text. One evidence selector selects every
occurrence remaining after those filters, subject to a bounded fan-out. At
least one occurrence must remain, and each selected occurrence becomes a
separate exact-range evidence link. Use the filters when only a subset of
repeated source text is intended. Public input never contains source offsets.
Once resolved, evidence supports the concept across all of its parent
relationships. Every leaf in the final projected corpus state must have at
least one evidence link.

### Operations

- `create_concept` requires request-unique `ref`, `label`, an unordered
  `parents` array, and nonempty `evidence`. An empty parent array creates a
  derived root. Labels may duplicate existing or newly created labels.
- `add_parent` ensures one broader-parent edge exists for `concept` without
  changing any other parent. An already-present edge is idempotent.
- `remove_parent` removes one parent edge without relocating the concept or its
  descendants. If it removes the final parent, the concept becomes a root.
- `add_evidence` ensures the evidence links selected by one or more quotations
  from the scoped work are attached to the selected concept. An
  already-satisfied mapping is idempotent.
- `remove_evidence` removes quotations from the scoped work that are attached
  to the selected concept.
- `reword_concept` preserves the public ID and requires
  `evidence_disposition: "retain" | "remove"`.
- `retire_concept` removes one concept and its incident edges. Retirement is
  nonrecursive: children survive, and a child with no remaining parents
  becomes a root. Optional `replacement` records a semantic successor but does
  not transfer edges or evidence.

The concept graph in the projected corpus state must be acyclic, have valid
endpoints and no self or duplicate edges, and have evidence on every leaf.
There is no parent priority, sibling placement, integer position, path, or move
operation.

## Local corpus browsing

Corpus reads are deliberately local and bounded. HEAD is the default; `--at`
selects an immutable historical revision.

```text
annals overview [--at REVISION]
annals roots [--at REVISION] [--limit N] [--cursor TOKEN]

annals concept show cN [--at REVISION] [--preview-limit N]
annals concept parents cN [--at REVISION] [--limit N] [--cursor TOKEN]
annals concept children cN [--at REVISION] [--limit N] [--cursor TOKEN]
annals concept evidence cN [--at REVISION] [--limit N] [--cursor TOKEN]

annals graph cN [--at REVISION] [--direction parents|children|both]
  [--depth N] [--max-nodes N]

annals search QUERY [--at REVISION] [--within cN]
  [--limit N] [--cursor TOKEN]
```

`overview` returns revision-wide counts for concepts, explicit edges, roots,
leaves, shared concepts, and evidence. It does not dump the graph.

`roots` pages through concept summaries with no parents. `concept show` returns
one concept's ID, label, relationship and evidence counts, derived
root/leaf/shared flags, and bounded previews. The `parents` and `children`
subcommands page through compact `{id, label}` references; `evidence` pages
through work-and-quotation pairs.

`graph` performs a bounded local expansion around one concept. `direction`
chooses incoming parent edges, outgoing child edges, or both. Each concept
appears once even when several routes reach it. When depth or node limits cut
off the expansion, the response reports a frontier instead of implying that
the returned neighborhood is complete. The response names its seed by ID,
stores each selected label once in `nodes`, and represents edges as
`{parent_id, child_id}` references into those nodes.

`search` matches labels and ancestor-label context. `--within cN` restricts the
search to the graph below one concept. Search results remain distinct by
public ID when labels repeat.

Paged responses contain `items` plus `page` with the requested limit,
returned count, total count, and optional `next_cursor`. The cursor is omitted
when the page is complete. Cursors are opaque and tied to the same library,
command, query, scope, and resolved revision. A later page may request a
different limit. Deterministic page order is a rendering contract, not a
conceptual ordering.

## Graph normalization

```text
annals shake [--yes]
```

`shake` computes the transitive reduction of HEAD. It removes an explicit
parent edge exactly when the child remains reachable from that parent through
another directed path. In interactive mode, the report gives the base revision,
edge counts before and after, and every edge that would be removed, then asks
once for confirmation. Only `y` or `yes`, case-insensitively, applies the plan;
any other answer or end-of-file cancels without writing. `--yes` bypasses the
prompt. With `--json`, omitting `--yes` returns the plan with status
`confirmation_required` and exit status zero, without writing. That preview is
informational: a later invocation with `--yes` computes and applies a fresh
plan for its then-current HEAD.

Within one invocation, a confirmed shake is bound to the persistent library
identity and the exact reported HEAD revision and graph. It applies every
reported removal and creates one `shake` commit in one transaction. If the
library identity, HEAD, or its graph changes before application, it fails with
`shake_stale` and removes nothing. A graph with no removable edges skips the
prompt and remains at its current revision.

Shaking preserves concepts, evidence, every ancestor-descendant pair, roots,
leaves, label/ancestor-context search matches and ranking, and `--within`
membership. It does not preserve every original path, direct-neighbor counts,
`shared` flags, hop distances, or the revision and direct-relationship metadata
included in search responses. Transitive reduction is optional rather than a
validation invariant; a later reconciliation may add shortcut edges again.

## History

```text
annals log [--limit N]
annals diff FROM TO
annals revert REVISION
```

`log` lists newest commits first. Work retention, recorded reconciliations,
model runs, and failed attempts are absent because they are not corpus
transitions. Applied reconciliations, confirmed shakes, and reverts are
commits.

`diff` replays two revisions and reports concept creation,
retirement, and rewording; individual parent edges added or removed; and
evidence added or removed. It never synthesizes a move or reorder event.

`revert` inverses one earlier commit against current HEAD and creates a new
commit. It does not erase history. If a relevant concept, edge, or evidence
fact has changed since the target transition, it fails atomically with
`revert_conflict`; unrelated relationships survive.

## Output and exit behavior

JSON success and failure envelopes are:

```json
{"ok":true,"data":{}}
```

```json
{"ok":false,"error":{"code":"stable_code","message":"description"}}
```

Public corpus JSON uses `cN` concept IDs, labels, exact quotations, edge
endpoints, opaque pagination cursors, and revision numbers. It does not expose
work, reconciliation, evidence, commit-row, or model-run IDs, nor source byte
ranges. Source-document heading paths remain public where they locate work
text.

Exit categories are:

| Code | Meaning |
| ---: | --- |
| 0 | Success |
| 1 | Unexpected process, I/O, or JSON failure |
| 2 | Invalid command or input |
| 3 | Missing library, work, concept, reconciliation, or revision |
| 4 | Stale state, invariant, or reversion conflict |
| 5 | SQLite, integrity, or history failure |

Human rendering escapes control characters from retained text and labels.

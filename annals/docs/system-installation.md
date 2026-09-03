# System installation and scheduled inbox

Annals can run as a scheduled, one-shot service around one configured library.
Each service activation registers settled inbox files, drains the
durable priority lane before the normal lane when dispatch is enabled, and
exits when no runnable work remains. Sequence remains FIFO within each lane. A
paused activation still registers settled files and leaves them queued. Annals
is not a resident daemon, contains no internal scheduler, and does not require
a separate database server.

The default installation connects both Annals and the separate `annals-usage`
CLI to Nucleus. Annals Usage calculates token consumption and reads account
limits live; it stores no companion database and never becomes part of the
Annals library or corpus.

## Operational model

The Annals library is the corpus source of truth. The spool is a visible
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
observes incoming material; pathname bytes break observation ties. Registration
moves the source into
`queued/` and preserves the sequence in a receipt with state
`queued`, `priority` set to `normal`, attempts zero, and no database
source-delivery record. Direct enqueue instead copies an explicitly selected
source into a complete queued envelope, leaves the original unchanged, and can
set `priority` to `priority`. Dispatch moves the lowest-sequence priority job,
or the lowest-sequence normal job when no priority job is queued, to
`processing`, changes its receipt state to `processing`, increments its attempts
to one, and starts the delivery record. A job receives no second processing
attempt.

Ordinary version-6 receipts have null retry provenance. A retry child's receipt
sets `retry_event_id`, `retry_ordinal`, `retry_of_job_id`, and
`retry_of_ingestion_id` together. It may also set `retry_reconciliation_id`
when the original attempt owns one exact reconciliation eligible for validation
and reuse. `retry_ordinal` is the zero-based position in the event's frozen
failure order. The first four fields are all present or all absent; a
reconciliation ID is never a license to adopt unrelated work history. Existing
version-5 receipts are accepted with null retry fields; a normal later receipt
rewrite emits version 6 while keeping job identity, attempt, priority,
source-delivery linkage, and terminal archive. Selecting a failed original for
retry does not itself rewrite that historical receipt.

`.run.lock` prevents overlapping ordinary or retry workers. `.control.lock` is
held only around registration, direct enqueue, queued-job priority changes,
ordinary or retry dispatch, retry-child publication, sequence allocation,
pause changes, interruption, and terminal disposition; it is never held during
liaison work. `.paused` is the operator-owned dispatch gate managed by
`annals inbox pause` and `resume`. A validated `interrupt.json` is a durable
request bound to its named processing job. The macOS deployer temporarily
creates `.maintenance` to make a running worker stop cleanly after its current
job and to prevent other spool mutation during cutover. These operational
files are not retained works. Do not create, remove, or edit their contents
directly.

The inbox storage gate is separate from those marker files. Before a queued
claim it requires `minimum_available_bytes` on both the library and spool
filesystems. It is derived from current filesystem state, creates no marker,
and therefore needs no `resume` when storage recovers.

One ordinary `inbox run` invocation:

1. takes the inbox lock;
2. performs no registration, enqueue, priority change, repair, or dispatch when
   maintenance is requested;
3. recovers queued and previously dispatched jobs;
4. registers every eligible visible top-level regular file as a queued job;
5. stops before attempting or dispatching a job when paused;
6. checks the library and spool storage reserve before every queued claim,
   leaving the job unattempted and exiting successfully when either filesystem
   is below it;
7. performs one authenticated account preflight before the first queued
   dispatch, leaving every job queued and unattempted if authentication is
   unavailable;
8. dispatches the lowest-sequence priority job, or the lowest-sequence normal
   job when the priority lane is empty, for its only attempt, then archives a
   fresh duplicate or integrates and applies a new work;
9. rescans and registers newly eligible arrivals between jobs; and
10. continues until the queue is empty, pause, maintenance, or low storage
    closes a gate, or an unexpected processing failure is terminalized and ends
    the activation nonzero.

There is no item or activation-lifetime limit. A continuing stream of eligible
arrivals can therefore keep one worker active indefinitely. Processing is
deliberately sequential so every new work sees the corpus revision produced by
the work before it. A priority arrival does not preempt a processing job, but a
continuing priority stream can starve the normal lane; Annals provides no
fairness or starvation protection. Duplicate recognition follows the same lane
ordering. Every job-processing error fails the source delivery and moves the
envelope to `failed/` on its first attempt. Known item-local source errors let
draining continue. An unexpected model, runner, or runtime processing failure
instead ends the activation nonzero after archival; successors remain queued
for the next activation. A failed job is never retried automatically; bounded
operator retry is a separate event described below.
Low storage differs from a failed job: no claim, attempt, delivery, or model run
starts. The one-shot worker exits and the next scheduled activation checks
again. A storage probe error fails closed with `storage_probe_failed` rather
than assuming that capacity is safe.

`annals inbox register` exposes the same admission phase without starting a
delivery. It can register arrivals while a different worker is processing a
job because the queue-control lock protects the short mutation. The periodic
`inbox run` activation normally provides registration without needing a second
schedule.

`annals inbox enqueue [--priority] FILE...` copies explicitly named regular
files directly into durable queued envelopes and leaves the originals in
place. The files receive immutable sequences in argument order and use the
normal lane unless `--priority` selects the priority lane. Enqueue bypasses
`incoming/` and settling; Annals completes each material copy and job receipt
before publishing the envelope for dispatch, avoiding a partial-copy admission
race. It rejects a copy whose projected result would cross the configured spool
reserve. It starts no source delivery.

`annals inbox prioritize JOB_ID...` changes named queued jobs to priority;
`annals inbox deprioritize JOB_ID...` changes them to normal. Both commands are
ordered against dispatch by the control lock, accept only jobs still under
`queued/`, and preserve job IDs and immutable sequences. Sequence—not argument
order—therefore determines relative order within the selected lane; promoting
an older normal job can place it before newer priority jobs. Repeating the same
lane choice is idempotent. A changed job affects the next claim but never the
processing job.

`annals inbox pause` establishes an ordered dispatch barrier. If dispatch
already won the control lock, that job is current and may finish; if pause won,
the job stays queued. Once `pause` returns, no later job can start. Scheduled
activations continue registering arrivals while paused and exit successfully.
`annals inbox resume` clears only `.paused`; it does not start a worker and
never clears `.maintenance`. It also refuses while a retry event is preparing,
running, or halted. Use an explicit `inbox run` for immediate ordinary work or
wait for the next Clockwork or systemd activation. Both commands are idempotent
when no retry event blocks resume, and an operator pause survives deployment.

Bounded retry is an attended, quiescent operation:

Only failures that reached retained-work identity are eligible. Correct and
redeliver a pre-retention source failure as a new job because its archive has
no durable digest that can prove unchanged retry material.

```sh
annals inbox pause
annals inbox status
annals inbox retry preview --from FIRST_FAILED_JOB --through LAST_FAILED_JOB
annals inbox retry start --from FIRST_FAILED_JOB --through LAST_FAILED_JOB \
  --reason "credential outage"
annals inbox retry status
```

Both job anchors are required and inclusive. Preview orders their failed
source deliveries by completion time and delivery ID, not by queue sequence,
and changes no state. A delivery already used as an original remains visible
but is marked ineligible with its prior event and any child provenance; start
rejects the whole interval rather than silently omitting it. Start requires the
pause to be set, no active job, and no maintenance marker. It freezes exactly
the previewed failure interval and runs fresh linked children in that order
while ordinary dispatch remains paused. The original failed envelopes stay in
`failed/`. Each retry child copies its original's unchanged source into the
normal spool directories; its receipt identifies the event and original job
rather than placing it in a separate retry directory. Its lane is an internal
publication detail: the event controls its order, and ordinary priority-change
commands reject it.

Start and continue run one authenticated account preflight before their first
zero-attempt child claim. A failed preflight halts the event while every
remaining child stays queued at attempts zero with no child delivery or model
run.

They also check the storage gate before each child claim. Insufficient storage
halts the event with `insufficient_storage`; inability to measure it halts with
`storage_probe_failed`. The child remains queued and unattempted. Retry is
attended, so correct the condition and use `retry continue`; the ordinary timer
does not continue the event.

A known item-local failure is recorded in the event and does not stop later
members. An unexpected model, runner, or runtime failure halts the event and
leaves later members not attempted. After correcting the cause, inspect the
report and continue only that event:

```sh
annals inbox retry status EVENT_ID
annals inbox retry continue EVENT_ID
```

`inbox interrupt` may target an active retry child. Either `--as failed` or
`--as skipped` archives that child with the requested outcome and halts the
event; a later `retry continue` advances only members that were not attempted.

Continue never gives an already failed or skipped retry child another attempt.
A further attempt for a failed child requires another explicit bounded event.
Resume ordinary dispatch only after the retry event is completed. Completion
can include item-local failed or skipped outcomes, so inspect the durable status
report rather than treating a zero process exit as "every child succeeded."

`annals inbox interrupt JOB_ID --as failed|skipped [--reason TEXT]` durably
requests that one specifically named processing job stop. The required job ID
prevents the request from selecting a successor if the observed job finishes
first. A failed disposition archives the envelope in `failed/`; a skipped
disposition gives its job receipt state `skipped` and archives it in `skipped/`.
Because its source delivery already started, a skipped job maps to delivery
status `failed` with error code `inbox_job_skipped`. Interruption does not set
`.paused`; pause first and then interrupt when later queued jobs must remain
queued. The command reports a conflict as too late if the named job already has
a durable terminal delivery outcome or an applied or recorded reconciliation.
A pending reconciliation may still be skipped before inbox automatic
application begins.

A fresh inbox job whose exact bytes select an existing work completes with
`duplicate` retention and delivery result `retained`. Its job receipt has state
`done` and `result_status` `retained`, and its envelope moves to `duplicates/`
without a model examination, reconciliation, commit, or revision change.
Explicit manual integration continues to examine the selected work. This
routing is prospective: Annals does not move or relabel historical terminal
envelopes or delivery records.

`settle_seconds` protects casual direct copies: a file is eligible only after
its modification time is old enough. Dotfiles, names ending in `.part`,
directories, and symlinks are counted as ignored rather than processed. For a
strict producer handoff, copy the file to a staging directory on the same
filesystem and atomically move the completed file into `incoming`.

An arrival that is still settling during the final rescan, or that races the
final empty check, waits for the next scheduled activation. The periodic
schedule is therefore a wake-up and recovery mechanism rather than a batch
boundary. Annals does not assign wall-clock run times or maintain another
scheduled-job concept internally.

## Configuration

The packaged Linux example is
[`packaging/systemd/annals.toml`](../packaging/systemd/annals.toml):

```toml
library = "/var/lib/annals/annals.db"

[inbox]
root = "/var/spool/annals"
settle_seconds = 60
minimum_available_bytes = 7_000_000_000

[liaison]
quality = "high"
nucleus_socket = "/var/lib/annals/Library/Application Support/Nucleus/nucleus.sock"
# model = "gpt-5.6-sol"
```

The companion has its own configuration. The packaged Linux example is
[`packaging/systemd/usage.toml`](../packaging/systemd/usage.toml), installed as
`/etc/annals/usage.toml`; macOS keeps `usage.toml` beside the Annals config:

```toml
nucleus = "/usr/local/bin/nucleus"
nucleus_socket = "/var/lib/annals/Library/Application Support/Nucleus/nucleus.sock"
library = "/var/lib/annals/annals.db"
spool = "/var/spool/annals"
```

Both configurations must select the same reachable Nucleus socket. The
`nucleus` executable in `usage.toml` is used only when `annals-usage login`
delegates to `nucleus auth login`; report, budget, doctor, and examinations use
the socket. Nucleus owns the Codex executable, persistent credential home, and
exclusive authentication lease. Annals and Annals Usage do not read or set
`CODEX_HOME`.

The core executable selects a configuration only from `--config`, then a
nonempty `ANNALS_CONFIG`. The library resolves from `--library`, then a
nonempty `ANNALS_LIBRARY`, then `library` in the selected config. If none of
those selects a library, the command fails; Annals does not search the current
directory or fall back to `./annals.db`.

The macOS frontend described below supplies its per-user config path when no
explicit config or library selection is present. The Linux units continue to
pass `/etc/annals/config.toml` explicitly and select the companion config with
`ANNALS_USAGE_CONFIG=/etc/annals/usage.toml`. Inbox-run options override their
corresponding config values. A relative `library` or `inbox.root` value is
resolved from the config file's directory. A manual Linux run can therefore
disable settling without editing the installed config:

```sh
ANNALS_USAGE_CONFIG=/etc/annals/usage.toml \
  annals --config /etc/annals/config.toml inbox run --settle-seconds 0
```

`settle_seconds = 0` makes every accepted regular file immediately eligible.
`minimum_available_bytes` defaults to 7,000,000,000 bytes and requires that
many bytes to remain available to the Annals user on both the library and spool
filesystems before a queued claim. Set it to zero to disable the gate. The
check is a reserve at each safe claim boundary, not a filesystem quota: an
active job or another process can still consume space afterward.
Unknown config keys are rejected.

The usage config resolves from an explicit `annals-usage --config`, then
`ANNALS_USAGE_CONFIG`, then `usage.toml` beside `ANNALS_CONFIG`, then the macOS
user-state default. Its relative paths are resolved from its own directory and
unknown keys are rejected. It has no `database` key: reports are calculated
from live Nucleus output plus the Annals library and spool. See [Consumption
telemetry](telemetry.md) for the reporting and accounting contract.

Use `model` only when an exact model override is wanted. Otherwise `quality`
selects Annals' model and reasoning preset. Annals defaults to `high` when
`quality` is omitted, and the packaged examples make that choice explicit. The
[reconciliation-v2 experiment](../experiments/04-twenty-chat-medium-high-reconciliation-v2/walkthrough.md)
averaged about 99 seconds per work at medium and 471 seconds at high over the
same 20 works. Those historical measurements are sizing guidance, not a
throughput promise. A single liaison has a 60-minute timeout to bound a hung
item, independently of the queue-draining activation's otherwise unbounded
lifetime. For an inbox job, a timeout without durable success is its terminal
processing failure. The activation exits nonzero after archiving it, its
successors wait for the next activation, and the timed-out job is not attempted
again.

## Linux with systemd

The packaged units use this layout:

```text
/usr/local/bin/annals
/usr/local/bin/annals-usage
/usr/local/bin/nucleus
/etc/annals/config.toml
/etc/annals/usage.toml
/var/lib/annals/
|-- annals.db
`-- Library/Application Support/Nucleus/
    `-- nucleus.sock          # supplied by the separately deployed Nucleus service
/var/spool/annals/
|-- .queue.json
|-- .run.lock
|-- .control.lock
|-- .paused       # present while operator-paused
|-- .maintenance  # present only during managed maintenance
|-- incoming/
|-- queued/
|-- processing/
|-- done/
|-- duplicates/
|-- failed/
`-- skipped/
```

The state directory must be writable by the service account because SQLite may
create WAL and shared-memory sidecars next to the Annals library. The Nucleus
socket must be reachable by the `annals` service account; deploy and validate
that service separately before enabling the inbox timer. The packaged paths
assume Nucleus also runs as `annals` with `HOME=/var/lib/annals`, so its
standard socket and the credential home selected by delegated login agree.

Build Annals, create a non-login service account, and install the files:

```sh
cargo build --release --package annals --package annals-usage

sudo groupadd --system annals
sudo useradd --system --gid annals --home-dir /var/lib/annals \
  --shell /usr/sbin/nologin annals
sudo install -m 0755 ../target/release/annals /usr/local/bin/annals
sudo install -m 0755 ../target/release/annals-usage \
  /usr/local/bin/annals-usage

sudo install -d -o root -g annals -m 0750 /etc/annals
sudo install -d -o annals -g annals -m 0700 /var/lib/annals
sudo install -d -o annals -g annals -m 0710 /var/spool/annals
sudo install -d -o annals -g annals -m 0770 \
  /var/spool/annals/incoming
sudo install -d -o annals -g annals -m 0700 \
  /var/spool/annals/queued \
  /var/spool/annals/processing \
  /var/spool/annals/done \
  /var/spool/annals/duplicates \
  /var/spool/annals/failed \
  /var/spool/annals/skipped

sudo install -o root -g annals -m 0640 \
  packaging/systemd/annals.toml /etc/annals/config.toml
sudo install -o root -g annals -m 0640 \
  packaging/systemd/usage.toml /etc/annals/usage.toml
sudo install -o root -g root -m 0644 \
  packaging/systemd/annals-inbox.service \
  /etc/systemd/system/annals-inbox.service
sudo install -o root -g root -m 0644 \
  packaging/systemd/annals-inbox.timer \
  /etc/systemd/system/annals-inbox.timer
```

Adjust the executable and socket paths in the configs and service if Nucleus
or Annals is installed elsewhere. Authenticate through the Nucleus service so
login, account reads, and model jobs share its credential lease:

```sh
sudo -u annals env HOME=/var/lib/annals \
  ANNALS_USAGE_CONFIG=/etc/annals/usage.toml \
  /usr/local/bin/annals-usage login --device-auth
```

The configured Nucleus service owns its private credential directory. Do not
give Annals a second Codex home or invoke Codex directly as a fallback.

Initialize the library and enable the timer:

```sh
sudo -u annals env HOME=/var/lib/annals \
  /usr/local/bin/annals --config /etc/annals/config.toml init

sudo -u annals env HOME=/var/lib/annals \
  /usr/local/bin/annals-usage doctor \
  --config /etc/annals/usage.toml

sudo systemctl daemon-reload
sudo systemctl enable --now annals-inbox.timer
```

The timer runs two minutes after boot and five minutes after the previous
service activation becomes inactive. `Type=oneshot` prevents systemd from
starting a second copy of the service, and the Annals inbox lock also protects
against a concurrent manual invocation. The unit deliberately has no systemd
start timeout because one liaison can run for up to 60 minutes and an
activation may continue draining for as long as runnable work remains. An
unexpected processing failure still ends that activation nonzero after the
failed job is archived.

The packaged service sets
`ANNALS_USAGE_CONFIG=/etc/annals/usage.toml`. Both configs select the same
Nucleus socket, and no companion ledger is opened.

Inspect or trigger it with:

```sh
sudo systemctl start annals-inbox.service
sudo systemctl status annals-inbox.timer annals-inbox.service
sudo journalctl -u annals-inbox.service
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox status
```

Human status output reports incoming files split into ready and settling,
the total queued count and its priority subset, processing, done, duplicate,
failed, and skipped envelopes; identifies the next and active jobs and their
priorities; and reports whether a worker is active and whether pause or
maintenance is requested. It also reports whether the live storage gate is
ready and its library and inbox measurements. `--json` exposes the subset as
`priority_queued` and
`priority` under `next_job` and `active_job`, plus `attempts`, `started_at`, and
`interrupt_requested` under `active_job`; it uses `duplicates` and `skipped`
for their archive counts and additionally reports the ignored-entry count. A
successful run reports
registered, attempted, applied, recorded, duplicates, failed, and skipped
counts; ready, queued, or processing work remaining; settling arrivals;
whether the runnable queue was drained; and whether pause, maintenance, or low
storage stopped dispatch. A paused or storage-gated queue is valid remaining
work, so `queue_drained` stays false. The spool root,
recovered-job count, effective settling interval, elapsed seconds, and ignored
count are also available with `--json`. A low-space scheduled run exits zero
but writes a concise diagnostic even under `--quiet`; probe failure exits
nonzero.

Control the worker without changing the external timer:

```sh
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox pause
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox register
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox enqueue --priority ./report.md
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox prioritize JOB_ID
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox deprioritize JOB_ID
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox resume
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox interrupt JOB_ID --as skipped \
  --reason "operator stopped the active job"
```

Pause lets the current delivery finish. The next job stays under `queued/`,
and later timer activations continue admitting settled arrivals without
dispatching them. Enqueue and priority changes also remain available while
paused. Resume permits the next activation to dispatch; it does not trigger the
service itself. Interrupt targets only the named processing job and does not
pause later dispatch.

Use `annals lately --channel inbox` for durable, time-windowed delivery
history. It includes both successful and permanently failed inbox material;
moving a terminal envelope back through recovery selects its original delivery
receipt rather than adding another history row. Registered or directly
enqueued jobs have not started a source delivery and therefore appear in
`inbox status`, not `lately`, until dispatch.

### Dropping files

For an explicit priority handoff that also keeps the source file in place, use:

```sh
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox enqueue --priority ./report.md
```

Do not also copy that file into `incoming/`; the enqueue command has already
created its queued job envelope.

A direct copy is supported by the settling interval:

```sh
sudo -u annals cp -n ./report.md /var/spool/annals/incoming/report.md
```

For an atomic handoff, use a private staging directory beside the inbox. The
final move is on the same filesystem and keeps the original basename:

```sh
sudo install -d -o annals -g annals -m 0700 /var/spool/annals/staging
sudo -u annals cp ./report.md /var/spool/annals/staging/report.md
sudo -u annals mv /var/spool/annals/staging/report.md \
  /var/spool/annals/incoming/report.md
```

Do not overwrite an existing inbox pathname. Reusing a basename with different
bytes may also conflict with the immutable work label derived from that name;
Annals records that job as failed rather than silently changing the label.

## macOS user Clockwork binding

The macOS installation belongs entirely to the logged-in user. This is the
important maintenance boundary: Annals, Nucleus, Clockwork, their definitions,
and all release files have exactly that user's
authority. Updating the complete application therefore needs no stored
administrator credential, privileged helper, or passwordless `sudo` rule.

The layout is:

```text
$HOME/.local/bin/annals                 # frontend -> current release
$HOME/.local/bin/annals-usage           # companion CLI -> current release
$HOME/Library/Application Support/Clockwork/ # definition, binding, history
$HOME/Library/Application Support/Chancery/providers/
|-- annals -> Annals current release share/chancery/annals
`-- annals-usage -> Annals current release share/chancery/annals-usage
$HOME/Library/Application Support/Annals/
|-- config.toml
|-- usage.toml
|-- annals.db
|-- log/
|-- backups/
|-- install/
|   |-- releases/RELEASE_ID/
|   |   `-- share/chancery/
|   |       |-- annals/
|   |       `-- annals-usage/
|   |-- current -> releases/RELEASE_ID
|   `-- previous -> releases/RELEASE_ID
`-- spool/
    |-- .queue.json
    |-- .run.lock
    |-- .control.lock
    |-- .paused       # present while operator-paused
    |-- .maintenance  # present only during managed maintenance
    |-- incoming/
    |-- queued/
    |-- processing/
    |-- done/
    |-- duplicates/
    |-- failed/
    `-- skipped/
```

The Annals frontend supplies the state-local config only when no explicit
config or library was selected. Both `config.toml` and `usage.toml` select the
already deployed Nucleus socket. Clockwork key `annals/inbox` runs an explicit
`/bin/sh` interpreter and release-local runner, both pinned by SHA-256; that
runner executes the sibling release payload as `annals --quiet inbox run`.
The runner establishes umask `077` before that exec so any child-created file
defaults remain private.
Its environment is scrubbed and explicitly contains only `HOME`, `USER`,
`LOGNAME`, and `ANNALS_CONFIG`. It does not set `CODEX_HOME` or resolve the
product through `current`, `PATH`, or `~/.local/bin`.
The definition requests run-at-load plus a 300-second interval, skips overlap,
and has no activation timeout. These are wake-up requests rather than launch
or queue-drain deadlines. Clockwork records the selected definition and process
outcome, while Annals status and run reports remain authoritative for inbox,
delivery, and corpus success; Clockwork does not ingest Annals log bodies.

The underlying per-user launchd projection is available only while the user is logged in. It resumes at
the next login after a logout or restart. A service that must run at the login
window needs a system LaunchDaemon and cannot also be fully maintained by an
unprivileged user.

### Nucleus-owned Codex authentication

Deploy Nucleus before Annals. During this migration, import the currently
signed-in Annals Codex home once; Nucleus copies its credential rather than
sharing that directory:

```sh
../target/release/nucleus service install \
  --daemon ../target/release/nucleusd \
  --codex-home "$HOME/Library/Application Support/Annals/codex-home"
../target/release/nucleus service status
../target/release/nucleus health
```

Nucleus creates and thereafter owns
`$HOME/Library/Application Support/Nucleus/codex-home`. The former Annals
credential directory is only the initial import source and is not a runtime
path or a synchronized backup. The first Annals deployment's doctor check verifies Nucleus
readiness and authentication. `annals-usage login --device-auth` delegates to
`nucleus auth login --device-auth`, which uses the same Nucleus credential lease
as daemon jobs and account reads.

#### Attended reauthentication

If authentication becomes unavailable, pause dispatch and wait until
`annals inbox status` reports no active job. Then renew and verify the
Nucleus-owned credentials:

```sh
annals inbox pause
annals inbox status
annals-usage login --device-auth
annals-usage doctor
```

Run one attended manual integration of a deliberately selected retained work
as the full liaison canary; omit `--apply`, and use `--reexamine` so an earlier
examination is not reused:

```sh
annals integrate --work KNOWN_WORK_LABEL --reexamine
annals inbox resume
annals inbox run # optional: dispatch immediately instead of waiting for Clockwork
```

The canary creates a new examination and reconciliation record, so choose its
work intentionally and inspect the resulting pending or recorded
reconciliation. On Linux, run the same sequence as the service account and set
`ANNALS_USAGE_CONFIG=/etc/annals/usage.toml` for `annals-usage`, as in the
installation commands above. A failed inbox account preflight does not consume
a job attempt: it exits before dispatch, leaving the envelope queued with no
delivery record. This makes credential loss programmatically containable, while
the attended device login remains the recovery step when Codex requires user
authorization.

If the credential outage was discovered only after a release had already
terminalized a stretch of jobs, keep the inbox paused after the canary and use
the bounded retry preview, start, and status sequence. Resume ordinary dispatch
only after that event completes. Never move those historical failed envelopes
back into the queue.

### Deploy or update

The deployer does not compile the Cell workspace or install Nucleus. `ci.sh`
checks only the two Annals packages and builds both Annals release executables
under the Cell root target directory:

```sh
./ci.sh
./packaging/launchd/deploy-user.sh \
  --binary "$PWD/../target/release/annals" \
  --usage-binary "$PWD/../target/release/annals-usage" \
  --nucleus "$HOME/.local/bin/nucleus" \
  --nucleus-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock" \
  --clockwork "$HOME/.local/bin/clockwork"
```

That same command is the normal unattended update operation. It preflights the
candidate binaries, configuration, and library before cutover.
Complete program releases contain Annals, the telemetry companion, frontend,
release-local inbox runner, unrendered Clockwork definition template, the
former LaunchAgent template retained only for exact ownership checks, updater,
both product-owned Chancery provider bundles, and a hash manifest. The runner,
templates, and both bundle hashes participate in release identity. Only after
that identity exists does the deployer render absolute release paths and exact
interpreter/runner hashes and register the definition inactive.
The `annals` and `annals-usage` provider selectors point through the one Annals
`current` release selector, so the two contracts cut over and roll back
together with their executables. The deployer owns only those two provider
selectors and preserves every other provider. It publishes them even when
Chancery is not installed; neither Annals runtime depends on Chancery.
Annals CI validates each bundle and requires its declared release to equal the
corresponding Annals or Annals Usage package version. `release.sh` bumps the
selected package and provider manifest together.

During an update the
deployer acquires its update lock and immediately writes the maintenance
marker, establishing the no-new-claim boundary before candidate preparation.
If `annals/inbox` has a selected definition, the deployer verifies the complete
current Annals release and compares every stored executable-definition field
with that release before disabling or replacing it. A same-key foreign
definition is left untouched. During first handoff it likewise disables and
removes only the exactly owned legacy LaunchAgent before any Clockwork switch.
Clockwork inspection and binding mutation are separate operations, not a
compare-and-swap. Concurrent same-user direct mutation of `annals/inbox`
during deployment or migration is unsupported; a detected change makes the
handoff fail closed and may retain the maintenance recovery gate.
An active worker is allowed to finish its
current delivery, then stops before claiming another; an idle worker stops
immediately. After the old service is quiescent, the deployer runs the
candidate's authenticated doctor check, makes a consistent Annals library
backup, and applies any supported schema migration. Only then does it
atomically switch the `current` selector. It updates both command links and
both configs inside the same rollback-protected deployment transaction, then
checks the installed library statistics and inbox state before switching the
Clockwork binding to the candidate definition. The operator-owned pause marker
is retained, so run-at-load does not dispatch queued jobs when the installation
was paused. A failure restores the old selectors and configs, then the exact
prior Clockwork definition only when its binding was enabled, or the legacy
LaunchAgent, never both. A prior absent or disabled binding stays disabled
without transient activation; its inactive selected digest may remain the
candidate digest. If that cannot be proved, Annals does not blindly mutate
scheduler state: it keeps maintenance in place, attempts cleanup only for
attributable scheduler state, removes its public selectors, and retains the
private rollback transaction.
The Annals library, spool, logs, pause state, and archives are retained.

Every successful update from an existing release also writes a durable
`rollback_snapshot` path into `install/last-update.json`. That private directory
contains the pre-cutover `config.toml`, `usage.toml`, prior schedule record,
any migration-era legacy LaunchAgent plist, and a
`rollback.json` naming the previous and replacement release selectors. A
post-commit rollback must restore those files together with the previous
release selector while the inbox is under maintenance; switching only
`install/current` is not sufficient when configuration schemas changed. The
deployment snapshot deliberately contains no credentials or reporting data.
Nucleus state is outside this rollback and remains forward-only. Pre-commit
failures continue to restore the configuration artifacts automatically from
the live transaction.

The version-4 deploy path invokes the candidate's additive `migrate` after the
backup and while the service is quiescent. It adds retry-event provenance to a
version-3 library without replacing its works, deliveries, reconciliations,
commits, spool, or archives. The rollback transaction retains the pre-migration
backup if candidate migration or cutover fails.

No operator timing or manual service stop is required. It is safe to run the
same deployment command while a delivery is in progress; by default the
deployer waits up to 3,900 seconds for the current liaison's 60-minute limit
plus headroom. The cutover itself occurs only after the inbox lock becomes
idle, so one delivery cannot straddle old and new Annals binaries.

Set `ANNALS_UPDATE_WAIT_SECONDS` to another nonnegative number to bound the
Annals lock poll after new schedule admission has stopped. It is not an
end-to-end deployment timeout: disabling a no-timeout Clockwork activation
waits for its child to finish naturally. `--no-start` is a synthetic packaging
option that installs and verifies without changing the selected Clockwork
binding or legacy launchd state; it is not the complete scheduled-installation
outcome.

Schema version 3 established the intentional boundary that cannot open an
older library. Version 4 migrates version 3 additively; it does not change the
older boundary. For a pre-version-3 installation, use the guarded fresh-state
operation after `ci.sh` is green:

```sh
./packaging/launchd/deploy-user.sh \
  --binary "$PWD/../target/release/annals" \
  --usage-binary "$PWD/../target/release/annals-usage" \
  --nucleus "$HOME/.local/bin/nucleus" \
  --nucleus-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock" \
  --clockwork "$HOME/.local/bin/clockwork" \
  --fresh-state
```

`--fresh-state` cannot be combined with `--no-start`. It stages an initialized
empty library and paused spool, verifies the paused spool, disables Clockwork
and any owned legacy LaunchAgent,
requests a graceful pause, waits for the current delivery, and registers all
remaining arrivals. Under
maintenance it moves the old library, WAL sidecars, and whole spool into one
directory under `backups/generations/` and switches in the fresh state. An
obsolete `usage.db` and its sidecars are held only inside the in-flight deploy
transaction for automatic rollback, then discarded when the deployment
commits; they are not part of the rollback generation.

Only after candidate and installed checks does the deployer import queued
and last-moment incoming sources from that generation. An attempted processing
job is terminalized rather than imported for another liaison run. The importer
preserves source bytes, priority choices, and lane sequence order while
assigning new unstarted delivery identities. It verifies the queued count,
clears the operator pause while maintenance still blocks dispatch, commits the
deployment receipt, removes maintenance, and wakes the worker. A pre-commit
failure puts the old generation and service back. A successful receipt records
`rollback_generation` and `imported_backlog` in
`install/last-update.json`; the archived generation remains available for
explicit recovery.

### Migrate the former system installation

The old root-owned LaunchDaemon layout requires one final attended migration.
Build the current release, then run the bundled migration while logged into the
operator's graphical session:

```sh
./ci.sh
sudo ./packaging/launchd/migrate-to-user.sh \
  --binary "$PWD/../target/release/annals" \
  --usage-binary "$PWD/../target/release/annals-usage" \
  --nucleus "$HOME/.local/bin/nucleus" \
  --nucleus-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock" \
  --clockwork "$HOME/.local/bin/clockwork"
```

The migration disables and drains `system/org.annals.inbox`, moves the whole
state directory on one filesystem so the database and its WAL sidecars stay
together, rewrites the two legacy absolute state paths, and performs the same
version-3 fresh-state cutover described above. The old database and spool are
kept as a rollback generation, while uncompleted sources enter the fresh inbox.
The child deployer keeps maintenance in place and returns a rendered Clockwork
definition without registering or selecting it. After verifying that inert
handoff, the outer migration durably records its committed phase, making the
user state and its content-addressed release permanent. Only then does it register the
definition, record its digest, and select `annals/inbox`; any immediate
RunAtLoad activation still observes maintenance. The migration next requires
`system/org.annals.inbox` to be absent, removes the superseded system files,
and only then clears maintenance. If a loaded service cannot be booted out or
remains visible, retirement fails closed with the files, transaction, and
maintenance marker retained.

A failure before the outer commit registers no definition and puts the
original state and system service back. A failure after that commit retains the
transaction, handoff, and maintenance marker; rerunning the migration
idempotently finishes registration and selection before clearing maintenance.
The system job and the Clockwork binding are never intentionally active
together; the inbox lock is a secondary guard, not scheduler coordination. The
migration accepts only an absent `annals/inbox` binding or a disabled tombstone
with no selected definition. Any selected digest, enabled or disabled, belongs
to another installation lifecycle and is left untouched. It also removes or
restores a legacy LaunchDaemon or LaunchAgent
only when the complete plist matches Annals' rendered template and its expected
owner and mode; matching only the label or executable is insufficient, and any
extra launchd key makes the file foreign.

### Operation and removal

Direct access and scheduler inspection need no elevation:

```sh
annals stats
annals inbox status
annals inbox pause
annals inbox register
annals inbox resume
annals inbox interrupt JOB_ID --as skipped --reason "operator request"
annals inbox retry preview --from FIRST_FAILED_JOB --through LAST_FAILED_JOB
annals inbox retry status
annals-usage report
annals-usage budget
annals-usage doctor
clockwork --json binding show annals/inbox
clockwork --json history annals/inbox --limit 20
tail -f "$HOME/Library/Application Support/Annals/log/inbox.stdout.log"
```

Submit a complete file under an unused name with:

```sh
install -m 0600 README.md \
  "$HOME/Library/Application Support/Annals/spool/incoming/annals-readme.md"
```

The Clockwork binding may remain enabled while paused. It continues to wake and
register settled arrivals, but the next job remains queued until `resume` and
a later activation. Run `annals inbox run` after `resume` when immediate
dispatch is wanted.
When only the storage gate is closed, no resume is needed: each 300-second
wake-up rechecks capacity and dispatches automatically once the reserve is
available.

Retiring the user installation requires the same fail-closed ownership checks
as deployment: a selected `annals/inbox` definition must exactly match the
current Annals release, a legacy LaunchAgent must exactly match Annals' fully
rendered plist, and every command or provider selector must have its exact
Annals target before it is disabled or removed. A same key, label, or pathname
is not ownership proof.

There is no supported raw path-only removal sequence. In particular, do not
run `clockwork binding disable annals/inbox`, `launchctl bootout` for the legacy
label, or `rm` against the installed selectors merely because those names are
present. Leave the installation intact until a product-owned retirement
operation has proved all of those identities. Such an operation must retain
the exact versioned release bytes alongside Clockwork definitions and
activation history; pruning either is a separate explicit lifecycle action.

Back up and explicitly remove the remaining state only when the library,
queued material, logs, and archives are no longer needed. Nucleus credentials
and raw model output belong to Nucleus's separate retention boundary.

## Failure recovery and maintenance

`annals inbox status` summarizes incoming, ready, settling, total queued and
priority-queued, processing, done, duplicates, failed, and skipped state and
identifies the next and active jobs with their priorities; `--json` also
includes ignored entries, `next_job`, and `active_job`. Inspect queued work
under `queued/JOB_ID/` and the active envelope under `processing/JOB_ID/`.
Inspect `failed/JOB_ID/job.json` or `skipped/JOB_ID/job.json` alongside the
unchanged source in that envelope's `material/` directory. Known item-local
source errors such as invalid UTF-8, empty material, unusable labels, and label
collisions fail the source delivery and move the job to `failed/` on its first
attempt; Annals then continues draining. Unexpected model, runner, and runtime
processing failures are also terminalized in `failed/` on the first attempt,
but the current activation exits nonzero and leaves successors queued for the
next activation.

Low storage is reported without terminalizing the next job. Its receipt stays
queued with attempts zero and no database delivery record. Free space on both
reported locations, then wait for the next scheduled activation or run
`annals inbox run` explicitly. If the report is `storage_probe_failed`, correct
the path or permission failure; do not bypass the check by editing spool state.

Storage remediation is a user decision, not authority granted to Annals, its
liaison, or another model or agent. They may report the affected paths and
available bytes, preserve the queued work, and wait for direction. They must
not, as storage remediation, delete, truncate, rotate, prune, move, compress,
overwrite, or otherwise clear library, spool, archive, backup, log, Nucleus, or
unrelated host data, or lower or disable `minimum_available_bytes`, without the
user's explicit consent for the exact target and scope. A request to run,
continue, retry, update, or deploy Annals authorizes only that operation's
documented effects, not additional cleanup. Scheduled activations may keep
performing their documented read-only capacity check and resume ordinary queued
work once the configured reserve is available; they perform no cleanup.

The storage gate is scoped to new ordinary and retry-child claims rather than
being a host-wide deployment lock. The related direct-enqueue headroom check
applies separately to an explicit spool copy. A gated inbox item does not
submit its associated liaison job to Nucleus. The gate does not itself block an
Annals deployment, globally stop the Nucleus service or independent Nucleus
jobs, or block another product's deployment; manual `annals integrate` is also
outside this gate. Those operations can still fail independently when the
underlying filesystem lacks enough real capacity for a build, release staging,
backup, migration, database write, or log write. The Annals deployer therefore
may proceed while the gate is closed, but it does not promise capacity or gain
cleanup authority by doing so. An unreadable storage probe is different from a
measured low-space result: the deployer's status inspection can fail with
`storage_probe_failed` until the path or permission problem is corrected.

Do not move failed envelopes back into `queued/` or edit their receipts. For a
recoverable stretch, use the bounded retry sequence above. `retry preview`
requires two failed-job anchors and reports the exact inclusive membership.
`retry start` records that frozen list, preserves every original delivery and
envelope, and creates a distinct linked child for each member. `retry status`
pairs each original failure with its child outcome, so the audit shows what was
and was not recovered.

An authenticated account preflight failure is earlier than job processing and
has different effects: no envelope is claimed, no attempt is recorded, and no
source delivery starts. The activation exits nonzero with the next job still
queued. Follow the [attended reauthentication](#attended-reauthentication)
sequence; do not move or repair the queued envelope.

Recovery never invokes a second liaison for a receipt whose attempts value is
already positive. It first finishes durable success from that attempt when
possible: for example, it can archive a conclusively retained duplicate or
finish the exact reconciliation linked through the job receipt and model-run
token. It does not adopt an unrelated reconciliation for the same work. If no
durable success exists, recovery fails the interrupted delivery and archives
the job. A durable interrupt request selects the requested failed or skipped
archive; a skipped job's source delivery is failed with
`inbox_job_skipped`. Startup also removes an empty envelope left before a claim
move and reconstructs a missing receipt when its envelope already contains
exactly one moved material file. If a worker is killed during examination,
recovery retires only the model-run token owned by that receipt and marks its
open reconciliation draft abandoned; it does not close a separate manual
examination of the same work.

Spool recovery also performs the queue-state migration from releases that had
no `queued/` directory. A legacy `processing/` envelope with zero attempts is
moved to `queued/` and receives the queued receipt state and immutable sequence;
an envelope with an attempt or other processing progress is recovered without
another liaison and then completed or failed according to its durable state.
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

On Linux, stop scheduling before maintenance that changes the executable,
configuration, or library:

```sh
sudo systemctl stop annals-inbox.timer
sudo systemctl start annals-inbox.timer
```

On macOS, `deploy-user.sh` coordinates this boundary with the maintenance
marker and restores scheduling automatically.

Use `inbox pause` for ordinary processing control instead of manipulating the
external timer or `.maintenance`. Pause continues admission and survives an
update; maintenance is reserved for a deployment boundary and blocks
registration, direct enqueue, priority changes, retry execution, and dispatch.
`inbox resume` never removes maintenance and refuses an unfinished retry event.
Use `inbox interrupt` for one active job; it remains independent of pause and
maintenance.

Use `annals backup` for a consistent SQLite backup rather than copying a live
WAL database. Include the spool when a
backup must preserve pending work, an unfinished retry event's child envelopes,
priority choices, and sequence order.
Keep the `done`, `duplicates`, `failed`, and `skipped` envelopes according to
the installation's retention policy; Annals does not silently delete source
files from these archives.

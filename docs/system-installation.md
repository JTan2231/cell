# System installation and scheduled inbox

Annals can run as a scheduled, one-shot service around one configured library.
Each service activation registers settled inbox files, drains the
durable priority lane before the normal lane when dispatch is enabled, and
exits when no runnable work remains. Sequence remains FIFO within each lane. A
paused activation still registers settled files and leaves them queued. Annals
is not a resident daemon, contains no internal scheduler, and does not require
a separate database server.

The default installation also runs liaison traffic through the separate
`annals-usage` CLI. Its companion SQLite ledger records token consumption and
account-limit snapshots without becoming part of the Annals library or corpus.

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

The current `job.json` receipt format is version 5 and is authoritative after
registration or direct enqueue. `.queue.json` assigns a UTC first-seen time and
prospective monotonic sequence when Annals first observes incoming material;
pathname bytes break observation ties. Registration moves the source into
`queued/` and preserves the sequence in a receipt with state
`queued`, `priority` set to `normal`, attempts zero, and no database
source-delivery record. Direct enqueue instead copies an explicitly selected
source into a complete queued envelope, leaves the original unchanged, and can
set `priority` to `priority`. Dispatch moves the lowest-sequence priority job,
or the lowest-sequence normal job when no priority job is queued, to
`processing`, changes its receipt state to `processing`, increments its attempts
to one, and starts the delivery record. A job receives no second processing
attempt.

`.run.lock` prevents overlapping workers. `.control.lock` is held only around
registration, direct enqueue, queued-job priority changes, dispatch, sequence
allocation, pause changes, interruption, and terminal disposition; it is never
held during liaison work. `.paused` is the operator-owned dispatch gate managed
by `annals inbox pause` and `resume`. A validated `interrupt.json` is a durable
request bound to its named processing job. The macOS deployer temporarily
creates `.maintenance` to make a running worker stop cleanly after its current
job and to prevent other spool mutation during cutover. These operational files
are not retained works. Do not create, remove, or edit their contents directly.

One invocation:

1. takes the inbox lock;
2. performs no registration, enqueue, priority change, repair, or dispatch when
   maintenance is requested;
3. recovers queued and previously dispatched jobs;
4. registers every eligible visible top-level regular file as a queued job;
5. stops before attempting or dispatching a job when paused;
6. dispatches the lowest-sequence priority job, or the lowest-sequence normal
   job when the priority lane is empty, for its only attempt, then archives a
   fresh duplicate or integrates and applies a new work;
7. rescans and registers newly eligible arrivals between jobs; and
8. continues until the queue is empty, pause or maintenance closes the gate,
   or an unexpected processing failure is terminalized and ends the activation
   nonzero.

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
for the next activation. The failed job is not retried.

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
race. It starts no source delivery.

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
never clears `.maintenance`. Use an explicit `inbox run` for immediate work or
wait for the next launchd or systemd activation. Both commands are idempotent,
and an operator pause survives deployment.

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

[liaison]
quality = "high"
codex = "/usr/local/bin/annals-usage"
# model = "gpt-5.6-sol"
```

The proxy has its own companion configuration. The packaged Linux example is
[`packaging/systemd/usage.toml`](../packaging/systemd/usage.toml), installed as
`/etc/annals/usage.toml`; macOS keeps `usage.toml` beside the Annals config:

```toml
codex = "/usr/local/bin/codex"
database = "/var/lib/annals/usage.db"
library = "/var/lib/annals/annals.db"
spool = "/var/spool/annals"
codex_home = "/var/lib/annals/codex-home"
```

Here `codex` means the real Codex executable, while `[liaison].codex` in the
Annals config means the proxy. The two paths must not point to the same
executable.

The core executable selects a configuration only from `--config`, then a
nonempty `ANNALS_CONFIG`. The library resolves from `--library`, then a
nonempty `ANNALS_LIBRARY`, then `library` in the selected config. If none of
those selects a library, the command fails; Annals does not search the current
directory or fall back to `./annals.db`.

The macOS frontend described below supplies its per-user config path when no
explicit config or library selection is present. The Linux units continue to
pass `/etc/annals/config.toml` explicitly and select the proxy config with
`ANNALS_USAGE_CONFIG=/etc/annals/usage.toml`. Inbox-run options override their
corresponding config values. A relative `library` or `inbox.root` value is
resolved from the config file's directory. A manual Linux run can therefore
disable settling without editing the installed config:

```sh
ANNALS_USAGE_CONFIG=/etc/annals/usage.toml \
  annals --config /etc/annals/config.toml inbox run --settle-seconds 0
```

`settle_seconds = 0` makes every accepted regular file immediately eligible.
Unknown config keys are rejected.

The usage config resolves from an explicit `annals-usage --config`, then
`ANNALS_USAGE_CONFIG`, then `usage.toml` beside `ANNALS_CONFIG`, then the macOS
user-state default. Its relative paths are resolved from its own directory and
unknown keys are rejected. The Annals library and `usage.db` are ordinary
state for the same installation and service user; no separate telemetry
service or database account is involved. See [Consumption
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
/etc/annals/config.toml
/etc/annals/usage.toml
/var/lib/annals/
|-- annals.db
|-- usage.db
`-- codex-home/
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
create WAL and shared-memory sidecars next to both databases.

Build Annals, create a non-login service account, and install the files:

```sh
cargo build --release

sudo groupadd --system annals
sudo useradd --system --gid annals --home-dir /var/lib/annals \
  --shell /usr/sbin/nologin annals
sudo install -m 0755 target/release/annals /usr/local/bin/annals
sudo install -m 0755 target/release/annals-usage \
  /usr/local/bin/annals-usage

sudo install -d -o root -g annals -m 0750 /etc/annals
sudo install -d -o annals -g annals -m 0700 \
  /var/lib/annals /var/lib/annals/codex-home
sudo install -d -o annals -g annals -m 0710 /var/spool/annals
sudo install -d -o annals -g annals -m 0770 \
  /var/spool/annals/incoming
sudo install -d -o annals -g annals -m 0700 \
  /var/spool/annals/queued \
  /var/spool/annals/processing \
  /var/spool/annals/done \
  /var/spool/annals/duplicates \
  /var/spool/annals/failed

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

Adjust the executable paths in the configs and service if Codex, Annals, or the
proxy is installed elsewhere. Authenticate real Codex into the service-owned
`CODEX_HOME`, then verify the login as that same account:

```sh
sudo -u annals env HOME=/var/lib/annals \
  CODEX_HOME=/var/lib/annals/codex-home \
  /usr/local/bin/codex \
  -c 'cli_auth_credentials_store="file"' login --device-auth

sudo -u annals env HOME=/var/lib/annals \
  CODEX_HOME=/var/lib/annals/codex-home \
  /usr/local/bin/codex \
  -c 'cli_auth_credentials_store="file"' login status
```

Keep this credential directory private. The service does not use an
administrator's personal Codex home. Annals copies `auth.json` from this
directory into each isolated liaison runtime, so manual installations force
file-backed Codex credentials rather than relying on an OS credential store.

Initialize the library and enable the timer:

```sh
sudo -u annals env HOME=/var/lib/annals \
  CODEX_HOME=/var/lib/annals/codex-home \
  /usr/local/bin/annals --config /etc/annals/config.toml init

sudo -u annals env HOME=/var/lib/annals \
  CODEX_HOME=/var/lib/annals/codex-home \
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
`ANNALS_USAGE_CONFIG=/etc/annals/usage.toml`, so the Annals process and every
proxy child use the same companion ledger without command-line intervention.

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
maintenance is requested. `--json` exposes the subset as `priority_queued` and
`priority` under `next_job` and `active_job`, plus `attempts`, `started_at`, and
`interrupt_requested` under `active_job`; it uses `duplicates` and `skipped`
for their archive counts and additionally reports the ignored-entry count. A
successful run reports
registered, attempted, applied, recorded, duplicates, failed, and skipped
counts; ready, queued, or processing work remaining; settling arrivals;
whether the runnable queue was drained; and whether pause or maintenance
stopped dispatch. A paused queue is valid remaining work, so `queue_drained`
stays false. The spool root,
recovered-job count, effective settling interval, elapsed seconds, and ignored
count are also available with `--json`.

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

## macOS user LaunchAgent

The macOS installation belongs entirely to the logged-in user. This is the
important maintenance boundary: Annals, the telemetry proxy and ledger, Codex,
its scheduler definition, and all release files have exactly that user's
authority. Updating the complete application therefore needs no stored
administrator credential, privileged helper, or passwordless `sudo` rule.

The layout is:

```text
$HOME/.local/bin/annals                 # frontend -> current release
$HOME/.local/bin/annals-usage           # CLI/proxy -> current release
$HOME/Library/LaunchAgents/org.annals.inbox.plist
$HOME/Library/Application Support/Annals/
|-- config.toml
|-- usage.toml
|-- annals.db
|-- usage.db
|-- codex-home/
|-- log/
|-- backups/
|-- install/
|   |-- releases/RELEASE_ID/
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
config or library was selected. That config points `[liaison].codex` at the
current release's `annals-usage` proxy, and `usage.toml` points the proxy at the
real Codex executable and the companion state paths. The LaunchAgent runs
`annals --quiet inbox run` with the user's real `HOME` and a private,
state-local `CODEX_HOME`. Add `$HOME/.local/bin` to `PATH` for interactive use;
launchd uses the absolute Annals frontend path.

This LaunchAgent is available only while the user is logged in. It resumes at
the next login after a logout or restart. A service that must run at the login
window needs a system LaunchDaemon and cannot also be fully maintained by an
unprivileged user.

### State-local Codex authentication

The deployer verifies existing state-local authentication but does not start an
interactive login. For a fresh installation, authenticate once before the
first deploy:

```sh
STATE_DIR="$HOME/Library/Application Support/Annals"
install -d -m 0700 "$STATE_DIR/codex-home"
HOME="$HOME" CODEX_HOME="$STATE_DIR/codex-home" \
  codex -c 'cli_auth_credentials_store="file"' login --device-auth
HOME="$HOME" CODEX_HOME="$STATE_DIR/codex-home" codex login status
```

Keep that directory private. Annals copies its `auth.json` into each isolated
liaison runtime rather than relying on an OS credential store. Migration from
the former system installation retains its existing state-local credentials.

### Deploy or update

The deployer does not compile the workspace or install Codex. `ci.sh` checks the
tree and builds both release executables:

```sh
./ci.sh
./packaging/launchd/deploy-user.sh \
  --binary "$PWD/target/release/annals" \
  --usage-binary "$PWD/target/release/annals-usage" \
  --codex "$(command -v codex)"
```

That same command is the normal unattended update operation. It preflights the
candidate binaries, configuration, library, and Codex login before cutover.
Complete program releases contain Annals, the telemetry proxy, frontend,
updater, rendered LaunchAgent, and a hash manifest. During an update the
deployer disables new activations and writes the maintenance marker. An active
worker is allowed to finish its current delivery, then stops before claiming
another; an idle worker stops immediately. The deployer then makes a consistent
Annals library backup, verifies the current schema, and atomically switches the
`current` selector. It updates both command links and both configs inside the
same rollback-protected deployment transaction, then validates through the
candidate and installed frontends before reloading launchd and waking the
worker. The operator-owned pause marker is retained, so that wake-up does not
dispatch queued jobs when the installation was paused. A failure restores the
old selectors, configs, plist, and service. Authentication, the Annals library,
telemetry ledger, spool, logs, pause state, and archives are retained.

No operator timing or manual service stop is required. It is safe to run the
same deployment command while a delivery is in progress; by default the
deployer waits up to 3,900 seconds for the current liaison's 60-minute limit
plus headroom. The cutover itself occurs only after the inbox lock becomes
idle, so one delivery cannot straddle the old and new proxy binaries.

Set `ANNALS_UPDATE_WAIT_SECONDS` to another nonnegative number when a caller
needs a shorter deadline. `--no-start` installs and validates without reading
or changing launchd state.

Schema version 3 intentionally cannot open an older library. For this specific
breaking boundary, use the guarded fresh-state operation after `ci.sh` is
green:

```sh
./packaging/launchd/deploy-user.sh \
  --binary "$PWD/target/release/annals" \
  --usage-binary "$PWD/target/release/annals-usage" \
  --codex "$(command -v codex)" \
  --fresh-state
```

`--fresh-state` cannot be combined with `--no-start`. It stages and validates
an empty library and paused spool, disables launchd, requests a graceful pause,
waits for the current delivery, and registers all remaining arrivals. Under
maintenance it moves the old library, WAL sidecars, usage ledger, and whole
spool into one directory under `backups/generations/` and switches in the fresh
state.

Only after candidate and installed validation does the deployer import queued
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
  --binary "$PWD/target/release/annals" \
  --usage-binary "$PWD/target/release/annals-usage" \
  --codex "$(command -v codex)"
```

The migration disables and drains `system/org.annals.inbox`, moves the whole
state directory on one filesystem so the database and its WAL sidecars stay
together, rewrites the two legacy absolute state paths, and performs the same
version-3 fresh-state cutover described above. The old database and spool are
kept as a rollback generation, while uncompleted sources enter the fresh inbox.
It then removes the validated old program files. If deployment fails, it puts
the original state and system service back. Do not run the old and new jobs
together: launchd domains allow identical labels, while the inbox lock only
prevents simultaneous workers.

### Operation and removal

Direct access and scheduler inspection need no elevation:

```sh
annals stats
annals validate
annals inbox status
annals inbox pause
annals inbox register
annals inbox resume
annals inbox interrupt JOB_ID --as skipped --reason "operator request"
annals-usage report
annals-usage budget
annals-usage doctor
launchctl print "gui/$(id -u)/org.annals.inbox"
tail -f "$HOME/Library/Application Support/Annals/log/inbox.stdout.log"
```

Submit a complete file under an unused name with:

```sh
install -m 0600 README.md \
  "$HOME/Library/Application Support/Annals/spool/incoming/annals-readme.md"
```

The LaunchAgent may remain loaded while paused. It continues to wake and
register settled arrivals, but the next job remains queued until `resume` and
a later activation. Run `annals inbox run` after `resume` when immediate
dispatch is wanted.

To retire the user installation while retaining its library, telemetry, and
operational state, boot out the LaunchAgent and remove only the scheduler,
command links, and versioned program directory:

```sh
launchctl bootout "gui/$(id -u)/org.annals.inbox" 2>/dev/null || true
rm -f "$HOME/Library/LaunchAgents/org.annals.inbox.plist"
rm -f "$HOME/.local/bin/annals"
rm -f "$HOME/.local/bin/annals-usage"
rm -rf "$HOME/Library/Application Support/Annals/install"
```

Back up and explicitly remove the remaining state only when the library,
telemetry ledger, credentials, queued material, logs, and archives are no
longer needed.

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
registration, direct enqueue, priority changes, and dispatch. `inbox resume`
never removes maintenance. Use `inbox interrupt` for one active job; it remains
independent of pause and maintenance.

Use `annals backup` for a consistent SQLite backup rather than copying a live
WAL database, and run `annals validate` periodically. Include the spool when a
backup must preserve pending work, its priority choices, and sequence order.
Keep the `done`, `duplicates`, `failed`, and `skipped` envelopes according to
the installation's retention policy; Annals does not silently delete source
files from these archives.

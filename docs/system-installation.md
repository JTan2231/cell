# System installation and scheduled inbox

Annals can run as a scheduled, one-shot service around one configured library.
Each service activation registers settled inbox files, drains the
durable queue in sequence, and exits when no runnable work remains. It is not
a resident daemon and does not require a separate database server.

## Operational model

The database is the corpus source of truth. The spool is a visible delivery
queue with a small Annals-owned ordering index:

```text
.queue.json
.run.lock
.maintenance
incoming/
`-- report.md
processing/
`-- JOB_ID/
    |-- job.json
    `-- material/
        `-- report.md
done/JOB_ID/       # the completed envelope
duplicates/JOB_ID/ # a fresh duplicate completed at retention
failed/JOB_ID/     # the permanently failed envelope
```

Annals moves the file into a unique job envelope on the same filesystem. It
does not rewrite its contents or basename. Moving the envelope to `done`,
`duplicates`, or `failed` prevents archive collisions without changing
`report.md`. The `material` subdirectory means even a source named `job.json`
cannot collide with the receipt. The same-filesystem moves preserve the
source's bytes, basename, inode, mode, and modification time.

`job.json` is the authoritative receipt after a job is claimed. `.queue.json`
assigns a monotonic sequence and UTC first-seen time when `inbox run` first
observes incoming material; pathname bytes break ties. The job receipt carries
that first-seen time, captured filesystem size and timestamps, and a stable key
for the corresponding database source-delivery receipt. `.run.lock` prevents
overlapping workers. The macOS deployer temporarily creates `.maintenance` to
make a running worker stop cleanly before taking another job. These are
Annals-owned operational files and are not retained works; do not edit them.

One invocation:

1. takes the inbox lock;
2. recovers the queue description for previously claimed jobs;
3. stops before processing another job when maintenance is requested;
4. registers every eligible visible top-level regular file as a durable job;
5. retains the oldest registered job, then archives a fresh duplicate or
   integrates and applies a new work;
6. rescans and registers newly eligible arrivals between jobs; and
7. continues until the queue is empty or a retryable failure stops the
   activation.

There is no item or activation-lifetime limit. A continuing stream of eligible
arrivals can therefore keep one worker active indefinitely. Processing is
deliberately sequential so every new work sees the corpus revision produced by
the work before it; duplicate recognition remains in the same FIFO. A retryable
failure remains at the head of the strict FIFO queue and stops that activation;
the next scheduled activation retries it before later work. Permanently invalid
material moves to `failed`, and draining continues.

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
boundary.

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
codex = "/usr/local/bin/codex"
# model = "gpt-5.6-sol"
```

The core executable selects a configuration only from `--config`, then a
nonempty `ANNALS_CONFIG`. The library resolves from `--library`, then a
nonempty `ANNALS_LIBRARY`, then `library` in the selected config. If none of
those selects a library, the command fails; Annals does not search the current
directory or fall back to `./annals.db`.

The macOS frontend described below supplies its per-user config path when no
explicit config or library selection is present. The Linux units continue to
pass `/etc/annals/config.toml` explicitly. Inbox-run options override their
corresponding config values. A relative `library` or `inbox.root` value is
resolved from the config file's directory. A manual Linux run can therefore
disable settling without editing the installed config:

```sh
annals --config /etc/annals/config.toml inbox run \
  --settle-seconds 0
```

`settle_seconds = 0` makes every accepted regular file immediately eligible.
Unknown config keys are rejected.

Use `model` only when an exact model override is wanted. Otherwise `quality`
selects Annals' model and reasoning preset. Annals defaults to `high` when
`quality` is omitted, and the packaged examples make that choice explicit. The
[reconciliation-v2 experiment](../experiments/04-twenty-chat-medium-high-reconciliation-v2/walkthrough.md)
averaged about 99 seconds per work at medium and 471 seconds at high over the
same 20 works. Those historical measurements are sizing guidance, not a
throughput promise. A single liaison has a 60-minute timeout to bound a hung
item, independently of the queue-draining activation's otherwise unbounded
lifetime.

## Linux with systemd

The packaged units use this layout:

```text
/usr/local/bin/annals
/etc/annals/config.toml
/var/lib/annals/
|-- annals.db
`-- codex-home/
/var/spool/annals/
|-- .queue.json
|-- .run.lock
|-- incoming/
|-- processing/
|-- done/
|-- duplicates/
`-- failed/
```

The database directory must be writable by the service account because SQLite
may create WAL and shared-memory sidecars next to the database.

Build Annals, create a non-login service account, and install the files:

```sh
cargo build --release

sudo groupadd --system annals
sudo useradd --system --gid annals --home-dir /var/lib/annals \
  --shell /usr/sbin/nologin annals
sudo install -m 0755 target/release/annals /usr/local/bin/annals

sudo install -d -o root -g annals -m 0750 /etc/annals
sudo install -d -o annals -g annals -m 0700 \
  /var/lib/annals /var/lib/annals/codex-home
sudo install -d -o annals -g annals -m 0710 /var/spool/annals
sudo install -d -o annals -g annals -m 0770 \
  /var/spool/annals/incoming
sudo install -d -o annals -g annals -m 0700 \
  /var/spool/annals/processing \
  /var/spool/annals/done \
  /var/spool/annals/duplicates \
  /var/spool/annals/failed

sudo install -o root -g annals -m 0640 \
  packaging/systemd/annals.toml /etc/annals/config.toml
sudo install -o root -g root -m 0644 \
  packaging/systemd/annals-inbox.service \
  /etc/systemd/system/annals-inbox.service
sudo install -o root -g root -m 0644 \
  packaging/systemd/annals-inbox.timer \
  /etc/systemd/system/annals-inbox.timer
```

Adjust the executable paths in the config and service if Codex or Annals is
installed elsewhere. Authenticate Codex into the service-owned `CODEX_HOME`,
then verify the login as that same account:

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

sudo systemctl daemon-reload
sudo systemctl enable --now annals-inbox.timer
```

The timer runs two minutes after boot and five minutes after the previous
service activation becomes inactive. `Type=oneshot` prevents systemd from
starting a second copy of the service, and the Annals inbox lock also protects
against a concurrent manual invocation. The unit deliberately has no systemd
start timeout because one liaison can run for up to 60 minutes and an
activation continues draining for as long as runnable work remains.

Inspect or trigger it with:

```sh
sudo systemctl start annals-inbox.service
sudo systemctl status annals-inbox.timer annals-inbox.service
sudo journalctl -u annals-inbox.service
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox status
```

Human status output reports incoming files split into ready and settling,
processing, done, duplicate, and failed envelopes, whether the run lock is
held, and whether maintenance is requested. `--json` uses `duplicates` for the
duplicate-archive count and additionally reports the ignored-entry count. A
successful run reports registered, attempted, applied, recorded, duplicates,
and failed counts; runnable work remaining; settling arrivals; whether the
runnable queue was drained; and whether it stopped for maintenance. The spool
root, recovered-job count, effective settling interval, elapsed seconds, and
ignored count are also available with `--json`.

Use `annals lately --channel inbox` for durable, time-windowed delivery
history. It includes both successful and permanently failed inbox material;
moving a terminal envelope back through recovery selects its original delivery
receipt rather than adding another history row.

### Dropping files

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
important maintenance boundary: Annals, Codex, its scheduler definition, and
all release files have exactly that user's authority. Updating the complete
application therefore needs no stored administrator credential, privileged
helper, or passwordless `sudo` rule.

The layout is:

```text
$HOME/.local/bin/annals                 # frontend -> current release
$HOME/Library/LaunchAgents/org.annals.inbox.plist
$HOME/Library/Application Support/Annals/
|-- config.toml
|-- annals.db
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
    |-- incoming/
    |-- processing/
    |-- done/
    |-- duplicates/
    `-- failed/
```

The frontend supplies the state-local config only when no explicit config or
library was selected. The LaunchAgent runs `annals --quiet inbox run` with the
user's real `HOME` and a private, state-local `CODEX_HOME`. Add
`$HOME/.local/bin` to `PATH` for interactive use; launchd uses the absolute
frontend path.

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

The deployer does not compile Annals or install Codex. `ci.sh` checks the tree
and builds the release executable:

```sh
./ci.sh
./packaging/launchd/deploy-user.sh \
  --binary "$PWD/target/release/annals" \
  --codex "$(command -v codex)"
```

That same command is the normal unattended update operation. It preflights the
candidate, configuration, library, and Codex login before cutover. Complete
program releases contain the payload, frontend, updater, rendered LaunchAgent,
and a hash manifest. During an update the deployer disables new activations,
requests maintenance, waits for the running worker to stop between jobs, makes
a consistent SQLite backup, applies any required schema migration, and switches
the `current` selector. It validates through the candidate and installed
frontends before reloading launchd. A failure restores the old selectors,
plist, and service. Configuration, authentication, the database, spool, logs,
and archives are retained.

The default drain deadline is 3,900 seconds, long enough for one liaison's
60-minute limit plus headroom. Set `ANNALS_UPDATE_WAIT_SECONDS` to another
nonnegative number when a caller needs a shorter deadline. `--no-start`
installs and validates without reading or changing launchd state.

### Migrate the former system installation

The old root-owned LaunchDaemon layout requires one final attended migration.
Build the current release, then run the bundled migration while logged into the
operator's graphical session:

```sh
./ci.sh
sudo ./packaging/launchd/migrate-to-user.sh \
  --binary "$PWD/target/release/annals" \
  --codex "$(command -v codex)"
```

The migration disables and drains `system/org.annals.inbox`, moves the whole
state directory on one filesystem so the database and its WAL sidecars stay
together, rewrites the two legacy absolute state paths, deploys the user
release, and removes the validated old program files. If deployment fails, it
puts the state and system service back. Do not run the old and new jobs
together: launchd domains allow identical labels, while the inbox lock only
prevents simultaneous workers.

### Operation and removal

Direct access and scheduler inspection need no elevation:

```sh
annals stats
annals validate
annals inbox status
launchctl print "gui/$(id -u)/org.annals.inbox"
tail -f "$HOME/Library/Application Support/Annals/log/inbox.stdout.log"
```

Submit a complete file under an unused name with:

```sh
install -m 0600 README.md \
  "$HOME/Library/Application Support/Annals/spool/incoming/annals-readme.md"
```

To retire the user installation while retaining its corpus and operational
state, boot out the LaunchAgent and remove only the scheduler, command link,
and versioned program directory:

```sh
launchctl bootout "gui/$(id -u)/org.annals.inbox" 2>/dev/null || true
rm -f "$HOME/Library/LaunchAgents/org.annals.inbox.plist"
rm -f "$HOME/.local/bin/annals"
rm -rf "$HOME/Library/Application Support/Annals/install"
```

Back up and explicitly remove the remaining state only when the corpus,
credentials, queued material, logs, and archives are no longer needed.

## Failure recovery and maintenance

`annals inbox status` summarizes incoming, ready, settling, processing, done,
duplicates, and failed state; `--json` also includes ignored entries. Inspect
`failed/JOB_ID/job.json` alongside the unchanged source in
`failed/JOB_ID/material/` to diagnose a permanent failure. Invalid UTF-8 and
empty material are permanent failures. For bytes not already retained,
unusable labels and label collisions are also permanent failures. Annals
archives them and continues draining. Other failures leave the
envelope in `processing`, stop the command with a nonzero exit, and are retried
at the head of the queue before later work on the next activation. On recovery,
the receipt's exact linked reconciliation lets Annals finish or archive
completed work without adopting an unrelated proposal for the same retained
work. A recovered envelope already linked to a reconciliation finishes that
historical work rather than being reclassified as a fresh duplicate.
Startup also removes an empty envelope left before a claim move and
reconstructs a missing receipt when its envelope already contains exactly one
moved material file.
If a worker is killed during examination, the next activation retires only the
model-run token owned by that receipt; it does not close a separate manual
examination of the same work.

Inbox delivery is at-least-once. A SQLite commit and the following receipt and
directory update cannot be one atomic transaction. If the process is killed in
that narrow interval, recovery may repeat retention handling and may examine a
new work again. The work's content-addressed storage keeps the exact source
bytes idempotent, but model examination for jobs that require it is not
guaranteed to run exactly once.

On Linux, stop scheduling before maintenance that changes the executable,
configuration, or library:

```sh
sudo systemctl stop annals-inbox.timer
sudo systemctl start annals-inbox.timer
```

On macOS, `deploy-user.sh` coordinates this boundary with the maintenance
marker and restores scheduling automatically.

Use `annals backup` for a consistent SQLite backup rather than copying a live
WAL database, and run `annals validate` periodically. Include the spool when a
backup must preserve pending work and its first-seen order. Keep the `done`,
`duplicates`, and `failed` envelopes according to the installation's retention
policy; Annals does not silently delete source files from these archives.

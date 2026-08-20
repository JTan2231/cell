# System installation and scheduled inbox

Annals can run as a scheduled, one-shot service around one system-owned
library. Each service activation registers settled inbox files, drains the
durable queue in sequence, and exits when no runnable work remains. It is not
a resident daemon and does not require a separate database server.

## Operational model

The database is the corpus source of truth. The spool is a visible delivery
queue with a small Annals-owned ordering index:

```text
.queue.json
.run.lock
incoming/
`-- report.md
processing/
`-- JOB_ID/
    |-- job.json
    `-- material/
        `-- report.md
done/JOB_ID/       # the completed envelope
failed/JOB_ID/     # the permanently failed envelope
```

Annals moves the file into a unique job envelope on the same filesystem. It
does not rewrite its contents or basename. Moving the envelope to `done` or
`failed` prevents archive collisions without changing `report.md`. The
`material` subdirectory means even a source named `job.json` cannot collide
with the receipt. The same-filesystem moves preserve the source's bytes,
basename, inode, mode, and modification time.

`job.json` is the authoritative receipt after a job is claimed. `.queue.json`
assigns a monotonic sequence when `inbox run` first observes incoming material;
pathname bytes break ties. `.run.lock` prevents overlapping workers. These are
Annals-owned operational files and are not retained works; do not edit them.

One invocation:

1. takes the inbox lock;
2. recovers a previously claimed job before claiming new work;
3. registers every eligible visible top-level regular file as a durable job;
4. integrates and applies the oldest registered job;
5. rescans and registers newly eligible arrivals between jobs; and
6. continues until the queue is empty or a retryable failure stops the
   activation.

There is no item or activation-lifetime limit. A continuing stream of eligible
arrivals can therefore keep one worker active indefinitely. Processing is
deliberately sequential because every work must see the corpus revision
produced by the work before it. A retryable failure remains at the head of the
strict FIFO queue and stops that activation; the next scheduled activation
retries it before later work. Permanently invalid material moves to `failed`,
and draining continues.

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

The macOS frontend described below supplies the system config path when no
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
processing envelopes, completed and failed envelopes, and whether the run lock
is held. `--json` additionally reports the ignored-entry count. A successful
run reports registered, attempted, applied, recorded, and failed counts;
runnable work remaining; settling arrivals; and whether the runnable queue was
drained. The spool root, recovered-job count, effective settling interval,
elapsed seconds, and ignored count are also available with `--json`.

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

## macOS with launchd

The macOS installation belongs to one explicitly selected local operator.
That same account owns every mutable file and runs the LaunchDaemon. This
single-UID model lets the operator, including Codex processes running as that
operator, use the complete CLI and SQLite library without `sudo` or shared-file
permission rules.

The installed program and state use this layout:

```text
/usr/local/bin/annals                    # system-default frontend
/usr/local/libexec/annals/annals         # Rust executable payload
/Library/LaunchDaemons/org.annals.inbox.plist
/Library/Application Support/Annals/
|-- config.toml
|-- annals.db
|-- codex-home/
|-- log/
`-- spool/
    |-- .queue.json
    |-- .run.lock
    |-- incoming/
    |-- processing/
    |-- done/
    `-- failed/
```

The state directory is private to the operator. The small root-owned frontend
does not elevate privileges or change the caller's identity. It executes the
payload with `/Library/Application Support/Annals/config.toml` when neither an
explicit config nor an explicit library was selected. `--config`,
`ANNALS_CONFIG`, `--library`, and `ANNALS_LIBRARY` continue to override that
default, so scratch and project-specific libraries remain easy to use.

The LaunchDaemon invokes the same frontend as `annals --quiet inbox run`.
launchd runs it as the operator with `HOME` set to the state directory and
`CODEX_HOME` set to its private `codex-home`. Interactive invocations keep the
caller's normal environment and Codex credentials; only their default Annals
config changes. Scheduled and interactive invocations therefore share a
corpus without sharing two Unix owners.

This trust boundary is deliberate: the scheduled Annals and Codex processes
run with the operator's filesystem authority. Select only the account intended
to own and maintain the consulting library.

### Bundled installer

The installer does not compile Annals or install Codex. Build and check the
release executable first, resolve Codex before invoking `sudo`, and name the
operator explicitly:

```sh
./ci.sh
sudo ./packaging/launchd/install.sh \
  --operator "$(id -un)" \
  --binary "$PWD/target/release/annals" \
  --codex "$(command -v codex)"
```

The installer accepts absolute executable paths and requires an existing local
operator account. It then:

- validates the operator, executable, templates, and fixed installation
  targets before changing the machine;
- creates private operator-owned state, log, Codex-home, and spool
  directories;
- installs the payload, system-default frontend, and rendered LaunchDaemon;
- creates the Annals and state-local Codex configuration when absent;
- checks Codex authentication as the operator in the state-local Codex home,
  using device authentication when login is required;
- initializes a missing library or validates an existing one as the operator;
  and
- loads and starts `system/org.annals.inbox` only after those checks succeed.

If authentication is cancelled or cannot run interactively, the installer
leaves the provisioned service disabled. Rerun the same command to finish. A
normal rerun with the same `--operator` is also the update path: it replaces
the payload, frontend, and project-owned launchd definition while retaining
configuration, the database, credentials, queued material, logs, and both
archives.

After installation, direct library access needs no `sudo` and works from any
directory:

```sh
annals stats
annals validate
annals overview
annals search "predicate locking"
annals inbox status
```

These commands also work from an unattended Codex process running as the
operator. An explicit target still bypasses the system default:

```sh
annals --library ./scratch.db init
annals --config ./project.toml stats
```

Scheduler administration remains a privileged launchd operation. Inspect its
definition and the operator-readable logs with:

```sh
sudo launchctl print system/org.annals.inbox
tail -f "/Library/Application Support/Annals/log/inbox.stdout.log"
tail -f "/Library/Application Support/Annals/log/inbox.stderr.log"
```

`RunAtLoad` handles startup and `StartInterval` requests another activation
every five minutes. launchd does not run another instance while the job is
still active; Annals' own lock remains authoritative for invocations outside
launchd. Each activation drains all runnable FIFO work, so the interval only
wakes an idle worker and provides retry and final-race recovery.

For example, the operator can submit this repository's README under a new
immutable work label without elevation:

```sh
install -m 0600 README.md \
  "/Library/Application Support/Annals/spool/incoming/annals-readme.md"
```

Choose an unused destination name. The default 60-second settling interval
ensures launchd does not claim a file still being copied; the next eligible
activation processes and applies it automatically.

### Uninstall

To remove scheduling and installed program files without deleting the corpus:

```sh
sudo ./packaging/launchd/uninstall.sh
```

The uninstaller disables and removes the LaunchDaemon, then removes the
frontend and executable payload. It deliberately retains everything under
`/Library/Application Support/Annals`, including configuration, the database,
state-local Codex credentials, logs, queued material, and both archives. The
operator is an existing user account and is never removed. Delete retained
state manually only after making any required backup.

### Manual installation

The bundled installer is the reference implementation for the fixed layout.
An MDM or other manual deployment must preserve the same invariants: install a
root-owned frontend separately from the payload; make all mutable state private
and writable by one named operator; render that operator into the LaunchDaemon;
set only the scheduled process's `HOME` and `CODEX_HOME` to state-local paths;
and have launchd call the frontend without a duplicated config argument. Load
the plist only after authentication, `annals validate`, and `annals inbox
status` all succeed as the operator.

## Failure recovery and maintenance

`annals inbox status` summarizes incoming, ready, settling, processing, done,
and failed state; `--json` also includes ignored entries. Inspect
`failed/JOB_ID/job.json` alongside the unchanged source in
`failed/JOB_ID/material/` to diagnose a permanent failure. Invalid UTF-8,
empty material, unusable labels, and label collisions are permanent failures;
Annals archives them and continues draining. Other failures leave the
envelope in `processing`, stop the command with a nonzero exit, and are retried
at the head of the queue before later work on the next activation. On recovery,
the receipt's exact linked reconciliation lets Annals finish or archive
completed work without adopting an unrelated proposal for the same retained
work.
Startup also removes an empty envelope left before a claim move and
reconstructs a missing receipt when its envelope already contains exactly one
moved material file.
If a worker is killed during examination, the next activation retires only the
model-run token owned by that receipt; it does not close a separate manual
examination of the same work.

Inbox delivery is at-least-once. A SQLite commit and the following receipt and
directory update cannot be one atomic transaction. If the process is killed in
that narrow interval, recovery may examine the retained work again. The work's
content-addressed storage keeps the exact source bytes idempotent, but model
examination itself is not guaranteed to run exactly once.

Stop scheduling before maintenance that changes the executable, configuration,
or library:

```sh
sudo systemctl stop annals-inbox.timer
sudo systemctl start annals-inbox.timer
```

Use `annals backup` for a consistent SQLite backup rather than copying a live
WAL database, and run `annals validate` periodically. Include the spool when a
backup must preserve pending work and its first-seen order. Keep the `done`
and `failed` envelopes according to the installation's retention policy;
Annals does not silently delete source files from either archive.

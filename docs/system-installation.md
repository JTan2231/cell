# System installation and scheduled inbox

Annals can run as a scheduled, one-shot service around one system-owned
library. The service polls a filesystem inbox, integrates a bounded batch in
sequence, and exits. It is not a resident daemon and does not require a
separate database server.

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
3. registers visible top-level regular files and snapshots those currently
   eligible;
4. attempts at most `max_items` from that snapshot, in persisted first-seen
   order;
5. integrates and applies each work before beginning the next one; and
6. exits after the item cap, the soft elapsed-time cap, an error that must be
   retried, or an empty queue.

New arrivals wait for the next invocation. The elapsed-time cap is checked
between items and never interrupts a liaison already running. An attempted
item counts toward `max_items`, including an item that is moved to `failed`.
Processing is deliberately sequential because every work must see the corpus
revision produced by the work before it.

`settle_seconds` protects casual direct copies: a file is eligible only after
its modification time is old enough. Dotfiles, names ending in `.part`,
directories, and symlinks are counted as ignored rather than processed. For a
strict producer handoff, copy the file to a staging directory on the same
filesystem and atomically move the completed file into `incoming`.

## Configuration

The packaged Linux example is
[`packaging/systemd/annals.toml`](../packaging/systemd/annals.toml):

```toml
library = "/var/lib/annals/annals.db"

[inbox]
root = "/var/spool/annals"
max_items = 5
max_elapsed_seconds = 2700
settle_seconds = 60

[liaison]
quality = "medium"
codex = "/usr/local/bin/codex"
# model = "gpt-5.6-sol"
```

Annals selects the configuration path from `--config`, then `ANNALS_CONFIG`;
it does not otherwise search for a config file. The library path resolves from
`--library`, then `ANNALS_LIBRARY`, then `library` in the selected config, then
`./annals.db`. Inbox-run options override their corresponding config values. A
relative `library` or `inbox.root` value is resolved from the config file's
directory. A manual run can therefore change the batch boundaries without
editing the installed config:

```sh
annals --config /etc/annals/config.toml inbox run \
  --max-items 1 --max-elapsed-seconds 600 --settle-seconds 0
```

`max_items` and `max_elapsed_seconds` must be positive; `settle_seconds = 0`
makes every accepted regular file immediately eligible. Unknown config keys
are rejected.

Use `model` only when an exact model override is wanted. Otherwise `quality`
selects Annals' model and reasoning preset. Annals defaults to `high` when
`quality` is omitted; the packaged examples deliberately choose `medium` for
routine queue processing. The
[reconciliation-v2 experiment](../experiments/04-twenty-chat-medium-high-reconciliation-v2/walkthrough.md)
averaged about 99 seconds per work at medium and 471 seconds at high over the
same 20 works. Those historical measurements are sizing guidance, not a
throughput promise. The five-item and 45-minute defaults bound ordinary
activations while allowing the current liaison to finish after the soft time
boundary.

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
  /usr/local/bin/codex login --device-auth

sudo -u annals env HOME=/var/lib/annals \
  CODEX_HOME=/var/lib/annals/codex-home \
  /usr/local/bin/codex login status
```

Keep this credential directory private. The service does not use an
administrator's personal Codex home.

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
start timeout because one liaison can run for up to 30 minutes and a batch can
contain several works.

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
run reports attempted, applied, recorded, and failed counts; ready or
processing work remaining; and one of `empty`, `batch_complete`, `max_items`,
or `max_elapsed` as its stop reason. Per-job results, recovered-job count,
effective limits, elapsed seconds, and ignored count are available with
`--json`.

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

The launchd example uses one dedicated, non-admin `_annals` account and keeps
all mutable state under `/Library/Application Support/Annals`:

```text
/usr/local/bin/annals
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

Provision the `_annals` user and group with local directory-management or MDM
tooling before loading the plist. If the service account or executable paths
differ, update both the plist and config example first.

Install the files and directories:

```sh
sudo install -m 0755 target/release/annals /usr/local/bin/annals
sudo install -d -o _annals -g _annals -m 0700 \
  "/Library/Application Support/Annals" \
  "/Library/Application Support/Annals/codex-home" \
  "/Library/Application Support/Annals/log" \
  "/Library/Application Support/Annals/spool" \
  "/Library/Application Support/Annals/spool/incoming" \
  "/Library/Application Support/Annals/spool/processing" \
  "/Library/Application Support/Annals/spool/done" \
  "/Library/Application Support/Annals/spool/failed"
sudo install -o root -g _annals -m 0640 \
  packaging/launchd/annals.toml \
  "/Library/Application Support/Annals/config.toml"
sudo install -o root -g wheel -m 0644 \
  packaging/launchd/org.annals.inbox.plist \
  /Library/LaunchDaemons/org.annals.inbox.plist
```

Authenticate and initialize as the service account:

```sh
sudo -u _annals env \
  HOME="/Library/Application Support/Annals" \
  CODEX_HOME="/Library/Application Support/Annals/codex-home" \
  /usr/local/bin/codex login --device-auth

sudo -u _annals env \
  HOME="/Library/Application Support/Annals" \
  CODEX_HOME="/Library/Application Support/Annals/codex-home" \
  /usr/local/bin/annals \
  --config "/Library/Application Support/Annals/config.toml" init
```

Load and inspect the LaunchDaemon:

```sh
sudo launchctl bootstrap system \
  /Library/LaunchDaemons/org.annals.inbox.plist
sudo launchctl enable system/org.annals.inbox
sudo launchctl kickstart -k system/org.annals.inbox
sudo launchctl print system/org.annals.inbox
```

`RunAtLoad` handles startup and `StartInterval` requests another activation
every five minutes. launchd does not run another instance while the job is
still active; Annals' own lock remains authoritative for invocations outside
launchd. Standard output and error go to the two files in the `log` directory.

## Failure recovery and maintenance

`annals inbox status` summarizes incoming, ready, settling, processing, done,
and failed state; `--json` also includes ignored entries. Inspect
`failed/JOB_ID/job.json` alongside the unchanged source in
`failed/JOB_ID/material/` to diagnose a permanent failure. Invalid UTF-8,
empty material, unusable labels, and label collisions are permanent failures;
Annals archives them and continues the batch. Other failures leave the
envelope in `processing`, stop the command with a nonzero exit, and are retried
before new incoming work on the next activation. On recovery, the receipt's
exact linked reconciliation lets Annals finish or archive completed work
without adopting an unrelated proposal for the same retained work.
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

# Install and recover Nucleus

This guide covers the current user's macOS service. Read the
[shared operator manual](operator-manual.md) before an operation that can
interrupt requesters or change a shared boundary.

## Paths and access

| Item | Default path |
| --- | --- |
| CLI | `~/.local/bin/nucleus` |
| Daemon | `~/.local/libexec/nucleusd` |
| Service | `~/Library/LaunchAgents/org.nucleus.daemon.plist` |
| Socket | `~/Library/Application Support/Nucleus/nucleus.sock` |
| Database | `~/Library/Application Support/Nucleus/nucleus.db` |
| Credential home | `~/Library/Application Support/Nucleus/codex-home/` |
| Program releases | `~/Library/Application Support/Nucleus/install/releases/` |
| Logs | `~/Library/Logs/Nucleus/` |

Treat state, credentials, logs, and backups as private. Database and mailbox
content can include prompts, source text, and tool values. Socket access uses
local ownership and permissions; protocol 1 has no TCP listener or separate
application authentication.

## Install or update

Use the CLI, daemon, and installer from the same sealed candidate:

```sh
<TESTED_NUCLEUS_INSTALL> install \
  --binary <TESTED_NUCLEUS_BINARY> \
  --daemon <TESTED_NUCLEUS_DAEMON> \
  --bundle /Users/joey/rust/cell/nucleus/chancery \
  --codex /absolute/path/to/codex
```

The exact Codex executable must be supported by the candidate adapter. For an
initial credential import, add `--codex-home /absolute/signed-in-codex-home`.
The source home is an import. Nucleus copies its credential into the private
owned home and uses a file credential store there.

The packaging installer stages the release and its version-matched Chancery
bundle. The service installer captures prior public programs, replaces the
LaunchAgent, and allows up to two minutes for migration, compaction, and health.
The daemon stays in the foreground under launchd.

A failed service cutover restores captured binaries and service configuration
only when the database schema is unchanged. A schema change prevents binary-only
rollback. Provider and release selectors recover with their programs.
Authentication is excluded from rollback.

After installation:

```sh
nucleus service status
nucleus health
nucleus account --wait 0
```

Verify the expected CLI, daemon, harness, and account before restoring requester
admission. `health` exits nonzero unless the service is compatible, authenticated,
and accepting jobs. The deployment owner's held-health interface is separate.

## Foreground development instance

From the Cell root:

```sh
cargo build --release --package nucleus-cli --package nucleus-daemon
target/release/nucleusd serve \
  --socket /tmp/nucleus.sock \
  --database /tmp/nucleus.db \
  --codex /absolute/path/to/codex \
  --codex-home /tmp/nucleus-codex-home
```

Select isolated paths and the supported harness explicitly. These example paths
must not already belong to another instance. Building does not install the user
service.

## Authentication recovery

Nucleus owns one canonical credential. Managed jobs receive access tokens in
memory, not refresh tokens. Static API-key jobs use isolated snapshots.
Account reads can overlap jobs. Canonical refresh is serialized and atomically
promotes validated staged credentials. A started refresh completes after its
requesting job is cancelled.

Attended login waits for active job and account sessions. Prevent new requester
work and let existing work settle, then run:

```sh
nucleus auth login --device-auth
nucleus account --wait 0
nucleus health
```

`annals-usage login --device-auth` delegates to the same operation. Resume only
the pauses created for this recovery after account and service checks pass.
Do not restore an older `auth.json` during program or database rollback.

## Back up state

Nucleus has no automatic backup or restore command. Select a private destination.

1. Quiesce requesters and wait for jobs to become terminal.
2. Record the Nucleus version, health, and exact Codex executable.
3. Stop the user service:

   ```sh
   launchctl bootout "gui/$(id -u)/org.nucleus.daemon"
   ```

4. Create a SQLite-aware backup of `nucleus.db`. Other copy methods must preserve
   the database and any WAL sidecars as one consistent set.
5. Back up the credential home separately only when credential recovery is required.
6. Include logs, service configuration, and requester state as needed. A Nucleus
   backup does not replace product backups.
7. Start the same service and check readiness:

   ```sh
   launchctl bootstrap "gui/$(id -u)" \
     "$HOME/Library/LaunchAgents/org.nucleus.daemon.plist"
   nucleus health
   ```

A copy of only the main database while it is live is not a complete backup.

## Restore state

Perform restoration with an operator present. Quiesce requesters and stop the
service. Save the current state separately before restoring a database with a
compatible binary. Check health and reads of retained jobs and output before
resuming admission. Recover credentials through their separate procedure.

### Schema recovery

Store schema 2 has an explicit version-one cutover. It preserves jobs, attempts,
registrations, cancellation, and terminal state. It discards the old mixed log
and historical answered mailbox rows. A pending call with a nonterminal owning
job and attempt prevents cutover; stale terminal-owner calls are discarded.

The transaction writes the new harness-output ledger and sets
`user_version=1000002`. This marker means compaction is pending. Each restart
retries `VACUUM` and a truncating WAL checkpoint. Only successful completion
publishes `user_version=2` and allows startup to continue. A failed compaction
remains pending and visible. Publishing the final marker can leave one bounded
WAL frame.

Version-one binaries cannot open schema 2. Recovery across the cutover requires
an explicit compatible database and binary pair. Keep credential recovery separate.

## Retention and removal

Monitor both state and logs:

```sh
du -sh "$HOME/Library/Application Support/Nucleus"
du -sh "$HOME/Library/Logs/Nucleus"
```

The LaunchAgent writes `nucleusd.stdout.log` and `nucleusd.stderr.log`.
Use the host's private-log rotation policy. Nucleus has no automatic output
pruning. Do not limit storage by deleting database rows, immutable registrations,
or individual credential-home files.

`nucleus service restart` terminates the daemon and asks launchd to start it
again. Quiesce first if attempts must finish. Startup marks unfinished attempts
`lost`; requester recovery decides what follows.

`nucleus service uninstall` removes installed Nucleus binaries and the LaunchAgent.
It retains state and logs. Removing retained material requires a separate decision
for that data and its recovery needs.

## Coordinated first installation and interrupted cutover

A fresh coordinated installation accepts `codex_bin` and `codex_home` in
Nucleus's deployment settings. Both paths are absolute. `codex_bin` must be the
exact supported Codex version. `codex_home` identifies an existing authenticated
home; settings never contain credential bytes. If Nucleus already owns valid
authentication, installation preserves it. Existing deployments retain their
captured harness and do not import another credential home.

The installer persists the run's local admission hold before a fresh daemon
exists. It starts the service under that hold so dependent products can finish
configuration. The configure phase proves the live harness, authentication,
protocol and drained capacity. Admission opens only at group release.

Before selector or service replacement, the installer writes private
`service-cutover.json` with its owner, prior package, candidate and harness.
Recovery uses this journal to select and reinstall the exact candidate through
`nucleus service recover --daemon ABS --codex ABS` with the recorded
`CELL_DEPLOYMENT_RUN_ID`. This controlled restart establishes which executable
is resident; matching files and a health response alone are insufficient.
The service must have its sole drained hold. If it is stopped, recovery reads
the database without migration and requires every retained job and attempt to
be terminal. It does not cancel or retry requester work.

Recovery can import the recorded authentication source only if Nucleus's owned
authentication file is absent. It never rolls back a credential or database.
A schema or service failure keeps the candidate and journal for recovery.
Unknown ownership or unfinished jobs keep admission held. Successful recovery
removes the cutover journal after held live health and exact program-copy checks.

Deployment settings accept only `codex_bin` and `codex_home`, both strings.
Unknown keys or values of another type fail inspection before admission holds.

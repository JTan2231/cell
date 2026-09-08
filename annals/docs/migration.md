# Migrate an older Annals installation

Schemas 3 through 5 use the additive migration in the
[installation guide](system-installation.md#deploy-or-update).
Use this page for a pre-version-3 library or a root-owned macOS installation.

## Replace a pre-version-3 library

After the required checks, run this command from the Annals directory:

```sh
../target/release/annals-install install \
  --binary "$PWD/../target/release/annals" \
  --usage-binary "$PWD/../target/release/annals-usage" \
  --bundle "$PWD/chancery/annals" \
  --usage-bundle "$PWD/chancery/annals-usage" \
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

## Migrate the former system installation

The old root-owned LaunchDaemon layout requires one final attended migration.
Build the current release, then run the bundled migration while logged into the
operator's graphical session:

```sh
./ci.sh
sudo ../target/release/annals-install migrate-to-user \
  --binary "$PWD/../target/release/annals" \
  --usage-binary "$PWD/../target/release/annals-usage" \
  --bundle "$PWD/chancery/annals" \
  --usage-bundle "$PWD/chancery/annals-usage" \
  --nucleus "$HOME/.local/bin/nucleus" \
  --nucleus-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock" \
  --clockwork "$HOME/.local/bin/clockwork"
```

The migration disables and drains `system/org.annals.inbox`, moves the whole
state directory on one filesystem so the database and its WAL sidecars stay
together, rewrites the two legacy absolute state paths, and performs the same
version-3 fresh-state cutover described above. The old database and spool are
kept as a rollback generation, while uncompleted sources enter the fresh
inbox.

The child deployer keeps maintenance in place and returns a rendered Clockwork
definition without registering or selecting it. After verifying that inert
handoff, the outer migration durably records its committed phase, making the
user state and its content-addressed release permanent. Only then does it
register the definition, record its digest, and select `annals/inbox`; any
immediate RunAtLoad activation still observes maintenance.

The migration next requires `system/org.annals.inbox` to be absent, removes
the superseded system files, and only then clears maintenance. If a loaded
service cannot be booted out or remains visible, retirement fails closed with
the files, transaction, and maintenance marker retained.

A failure before the outer commit registers no definition and puts the
original state and system service back. A failure after that commit retains
the transaction, handoff, and maintenance marker; rerunning the migration
idempotently finishes registration and selection before clearing maintenance.
The system job and the Clockwork binding are never intentionally active
together; the inbox lock is a secondary guard, not scheduler coordination.

The migration accepts only an absent `annals/inbox` binding or a disabled
tombstone with no selected definition. Any selected digest, enabled or
disabled, belongs to another installation lifecycle and is left untouched.

It also removes or restores a legacy LaunchDaemon or LaunchAgent only when the
complete plist matches Annals' rendered template and its expected owner and
mode; matching only the label or executable is insufficient, and any extra
launchd key makes the file foreign.
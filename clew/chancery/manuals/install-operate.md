# Install and verify Clew

Clew is an on-demand local CLI with an independent private application ledger.
It requires Platter opportunity read contract one for candidate search and first
reference admission. It installs no service, schedule or model requester.

## Deploy

Validate the changed product scope with the default root gate. Commit the green
candidate to local main. Preview and run the authorized deployment:

```sh
./ci.sh
./deploy.sh plan clew
./deploy.sh clew
```

Clew accepts no deployment settings. The coordinator installs a compatible
Platter reader first when necessary. Configure initializes an empty Clew ledger
or checks the existing schema. Verify checks the ledger's integrity and reads
retained Platter opportunity metadata. Deployment creates no application reports
and performs no preparation or send.

The immutable release contains `clew`, `clew-install`, the recovery installer,
and the matching Chancery provider. They live under
`~/Library/Application Support/Clew/install/releases/HASH`. The shared
`cell-install-v2` transaction atomically selects the public commands at
`~/.local/bin` and the Clew provider under Chancery. Foreign selectors, changed
release bytes and stale expected selections stop publication.

Direct program installation is supported with a sealed candidate:

```sh
clew-install install --binary /absolute/candidate/clew --bundle /absolute/clew/chancery
clew init
clew doctor
clew --register-usage
```

The installer accepts `--home ABSOLUTE_PATH` and an exact
`--expected-current absent|releases/HASH` condition. Direct installation selects
program files only; initialize state separately. Registration records command
identities in Chancery and adds no Clew reports.

## State and inspection

```sh
clew init
clew doctor
clew-install inspect
clew-install verify --binary /absolute/candidate/clew --bundle /absolute/clew/chancery
clew-install verify-release /absolute/owned/release
```

The ledger is `~/.local/share/clew/ledger.sqlite3`. Its directory must be private
(0700) and its database must be a private regular file (0600). Global
`--state-dir ABSOLUTE_PATH` selects another explicit private ledger for ordinary
commands. Deployment configures only the default ledger.

Init creates schema one in an empty database. It can finish initialization after
an interruption left an empty database. It preserves compatible existing rows
and refuses nonempty foreign or unsupported state. Doctor checks the local schema,
SQLite integrity and correction references. It returns the retained entry count
without creating entries or probing Platter. Installation inspection describes
files only; it does not establish future runtime readiness.

## Recovery

Each append is atomic. Reinvoke an uncertain write with its original ID and
identical arguments. Never edit SQLite to repair an entry. Use the application
contract's append-only correction or retraction commands.

Program recovery can select a retained version-two release:

```sh
clew-install recover --release /absolute/owned/release
```

File recovery preserves the separate ledger and does not restore or delete its
rows. Only schema-one-compatible programs can operate this ledger. No older
installation format or incompatible state migration is supported. Preserve
unresolved installation evidence if recovery fails. Coordinator recovery checks
any existing ledger before reporting safe program recovery.

For a filesystem backup or restore, stop Clew invocations and wait for all current
commands to finish. Preserve the database and any SQLite sidecars together in a
private backup. Keep the previous complete backup when restoring a compatible
history. Restoring an older history changes which write IDs are known; reconcile
uncertain writes from their retained receipts before replaying them. Clew supplies
no automatic backup, pruning or deletion operation.

Compatible schema-one commands can finish across program selection. An
incompatible future migration requires a separate procedure that excludes
writers and preserves a compatible backup. No duration guarantee is supplied.

Private notes belong in the ledger, outside the source and release trees.
Installation, initialization and catalog discovery authorize no application
status, employer contact, email or scheduled activation.

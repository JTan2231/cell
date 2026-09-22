# Install and verify Clew

Clew has an independent private application ledger and an optional daily email.
It requires Platter opportunity read contract one for candidate search and first
reference admission. Email contract four supplies mail submission. Clockwork
contract three supplies scheduled activation. Clew runs no model requester.

## Deploy

Use `cell-ci submit COMMIT` for ordinary CI delivery. The manager integrates,
validates, attempts bounded repairs, deploys, and emails the outcome. For a
separate authorized manual deployment, select a validated candidate on local
main, then preview and run the coordinator:

```sh
./deploy.sh plan clew
./deploy.sh clew
```

Clew accepts the boolean deployment setting `daily_email_enabled`. Omission
preserves existing schedule intent; an absent binding remains absent. Explicit
true authorizes daily submission of the content in the
[email contract](digest-email.md). Explicit false prepares a disabled selection.
The coordinator installs compatible Platter, Email and Clockwork dependencies
first when necessary. Configure initializes an empty ledger or checks its schema.
Verify checks ledger integrity and reads retained Platter opportunity metadata.
Deployment creates no application reports and performs no preparation or send.

The lifecycle captures `clew/daily-email`, holds email admission, suspends the
binding and drains admitted sends. Configure prepares a disabled definition for
the exact installed release. Activation restores captured or explicit enabled
intent after releasing maintenance. Existing schedule policy, disabled intent
and failure halts are preserved. No lifecycle operation approves an incident.
Application ledger reads and short writes can continue during email maintenance.

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

Prepare a definition without registration or activation:

```sh
clew-install schedule-definition --state-dir ABS_STATE --output ABS_NEW_FILE
```

The definition uses local 09:00, no run-at-load, skipped overlap, a 180-second
limit and `halt-until-approved`. The output must be a new absolute file. The
selected release and initialized ledger must already exist. Schedule generation
creates private logs but does not send. Actual timing depends on login and sleep.

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

Preserve `email.sqlite3`, its SQLite sidecars and `deployment-maintenance/` with
the ledger. The first send creates email schema one separately. Program recovery
does not erase occurrences or retry uncertain sends. Older Clew releases do not
understand daily email state: disable the daily binding and settle admitted sends
before an explicit rollback to such a release. Preserve all delivery records.
Coordinated recovery keeps email admission held until program and schedule
selection are coherent. Keep unresolved holds for coordinator recovery.

For a filesystem backup or restore, disable the daily schedule, stop Clew
invocations and wait for current commands to finish. Preserve both databases,
maintenance state and SQLite sidecars together in a
private backup. Keep the previous complete backup when restoring a compatible
history. Restoring an older history changes which write IDs are known; reconcile
uncertain writes and email acceptance before replaying them. Restoring older
delivery history can permit duplicate mail. Clew supplies
no automatic backup, pruning or deletion operation.

Compatible schema-one commands can finish across program selection. An
incompatible future migration requires a separate procedure that excludes
writers and preserves a compatible backup. No duration guarantee is supplied.

Private notes belong in the ledger, outside the source and release trees.
Installation, initialization and catalog discovery authorize no application
status, employer contact, email or scheduled activation.

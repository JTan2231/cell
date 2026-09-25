# Install and verify Clew

Clew has an independent private application ledger and an optional daily email.
It requires Cast discovery read contract two for candidate search, first-job
admission and email context. Platter opportunity read contract one supplies only
the mapping for a schema-one ledger migration. Email contract four supplies mail
submission. Clockwork contract three supplies scheduled activation. Clew runs no
model requester.

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
The coordinator installs compatible Cast, Email and Clockwork dependencies
first when necessary. Configure initializes an empty ledger, migrates a schema-one
ledger, or checks schema-two state. Verify checks ledger integrity and reads the
complete retained Cast snapshot.
Deployment creates no application reports and performs no preparation or send.

The lifecycle captures `clew/daily-email`, holds email admission, suspends the
binding and drains admitted sends. Configure prepares a disabled definition for
the exact installed release. Activation restores captured or explicit enabled
intent after releasing maintenance. Existing schedule policy, disabled intent
and failure halts are preserved. No lifecycle operation approves an incident.
Schema-two ledger reads and short writes can continue during email maintenance.
Schema-one migration uses the guarded procedure below.

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

Init creates schema two in an empty database. It can finish initialization after
an interruption left an empty database. It preserves compatible existing rows
and refuses nonempty foreign or unsupported state. Doctor checks the local schema,
SQLite integrity and correction references. It returns the retained entry count
without creating entries or probing Cast. Installation inspection describes
files only; it does not establish future runtime readiness.

## Migrate a schema-one ledger

Use coordinated deployment for this migration. Ordinary commands and `init`
refuse schema-one state; direct program installation does not migrate it.

Configure requires its sole owned email maintenance hold and drained sends.
It reads the Platter public opportunity interface and maps every legacy reference
retained in the ledger to its exact `cast_job_id`. Missing mappings or two legacy
references that select the same Cast job stop the migration. Clew does not infer
a mapping from company, role or URL, and does not merge histories.
An empty legacy ledger needs no Platter read.

Migration excludes concurrent ledger writes and creates a private, consistent
schema-one SQLite backup under the state root as
`ledger-schema1-backup-RUN_ID-UUID.sqlite3`, with mode 0600. It refuses to overwrite
an earlier backup. An interrupted retry creates a new backup. It then commits
the schema and alias mappings in one transaction.
After commit, old schema-one writers are rejected. Preserve the backup and the
reported recovery evidence until the deployment is verified.

Original ledger rows, write IDs, sequence, timestamps, supplied text, corrections
and retractions remain unchanged. Legacy aliases retain their exact argument
identity for retries. New records use Cast IDs. Frozen email occurrences, message
bytes, send keys and acceptance receipts remain unchanged. Migration creates no
application report and starts no collection, preparation or send.

## Recovery

Each append is atomic. Reinvoke an uncertain write with its original ID and
identical arguments. Never edit SQLite to repair an entry. Use the application
contract's append-only correction or retraction commands.

Program recovery can select a retained `cell-install-v2` release:

```sh
clew-install recover --release /absolute/owned/release
```

File recovery preserves the separate ledger and does not restore or delete its
rows. Only schema-two-compatible programs can operate a migrated ledger. Recovery
must not declare an old program compatible with schema-two state. No older
installation format or automatic schema downgrade is supported. Preserve
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
delivery history can permit duplicate mail. The schema-one migration creates its
required ledger backup. Clew supplies no general automatic backup, pruning or
deletion operation.

Keep a schema-two-capable release selected after migration. A rollback to a
schema-one program requires a matching schema-one ledger backup and stopped
commands and scheduling. Reconcile all post-migration reports and uncertain
email acceptance before restoring that backup. Preserve delivery state; an old
ledger backup does not justify restoring older email occurrences.

Compatible schema-two commands can finish across program selection. An
incompatible future migration requires a separate procedure that excludes writers
and preserves a compatible backup. No duration guarantee is supplied.

Private notes belong in the ledger, outside the source and release trees.
Installation, initialization and catalog discovery authorize no application
status, employer contact, email or scheduled activation.

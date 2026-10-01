# Private state and installation lifecycle

Clew owns its private ledger, retained email occurrences, program selection,
and email admission maintenance. This feature explains their lifecycle and
compatibility. Use `chancery show clew.install.operate` for ordered installation,
migration, schedule preparation, backup, and recovery procedures.

## Interfaces and ownership

```sh
clew init
clew doctor
clew-install inspect
clew-install install --binary /absolute/candidate/clew --bundle /absolute/clew/chancery
clew-install recover --release /absolute/owned/release
```

The current user owns the local installation and state. Installation,
initialization, inspection, and catalog discovery grant no authority to record
status, contact an employer, send mail, or enable scheduled activation.

| Item | Default path |
| --- | --- |
| Ledger | `~/.local/share/clew/ledger.sqlite3` |
| Email occurrences | `~/.local/share/clew/email.sqlite3` |
| Deployment maintenance | `~/.local/share/clew/deployment-maintenance/` |
| Immutable releases | `~/Library/Application Support/Clew/install/releases/HASH` |
| Public commands | `~/.local/bin/clew`, `~/.local/bin/clew-install` |
| Provider selector | `~/Library/Application Support/Chancery/providers/clew` |

Global `--state-dir ABSOLUTE_PATH` selects an independent private ledger for
ordinary Clew commands. Deployment configures only the default ledger. The state
directory has mode 0700; the databases are regular files with mode 0600. Keep
notes, statuses, thread names, references, frozen messages, and backups private.

The `clew.ledger.use` feature owns general entry meaning, named threads,
external references, append transactions, exact retry identity, and corrections.
The `clew.application.track` feature owns explicit application reports and the
job-specific view. Plain external job links do not create application reports.

The `clew.digest.email` feature owns message bytes, occurrences, receipts,
submission recovery, and email schema-one behavior. These records have no
automatic pruning. Clew has no general automatic backup or deletion operation.

## Initialization and observations

`clew init` and `clew doctor` print readable results by default. Use `--json`
for the schema-three machine response. Human errors go to stderr. JSON errors
retain `ok: false` and `error.detail` on stdout, with a nonzero exit.

Init creates ledger schema three in an empty database. It can finish initialization
when an interruption left an empty database. It preserves compatible existing
rows and refuses nonempty foreign or unsupported state. Ordinary commands never
initialize or migrate state implicitly. Init and ordinary commands refuse
schema-one and schema-two ledgers.

Doctor checks the local schema, SQLite integrity, and correction references. It
returns the retained entry count without adding entries or probing Cast.
Installation inspection describes selected release metadata. It does not
check artifact integrity or establish readiness.

Coordinated deployment creates or migrates state and selects program files. It
does not invoke doctor, check release integrity, or probe the Cast snapshot.

## Program and provider selection

A release contains `clew`, `clew-install`, the recovery installer, and the complete
matching Chancery bundle. Its content hash identifies the immutable release;
package version, feature contract version, and database schema are separate
identities. Provider pages and overview participate in release identity and
release selection.

The shared `cell-install-v2` transaction atomically selects public commands and
the Clew provider. Foreign selectors and stale expected selections stop publication. The installer accepts `--home ABSOLUTE_PATH` and
`--expected-current absent|releases/HASH`. Direct installation selects program
files only. It does not initialize or migrate state.

Program recovery selects a retained `cell-install-v2` release and
preserves separate ledger and email state. It does not restore, delete, or rewrite
rows, retry uncertain mail, or approve a scheduling incident. No older
installation format or automatic schema downgrade is supported. Preserve
unresolved installation evidence when recovery fails.

## Coordinated email maintenance

Deployment captures `clew/daily-email`, holds new email admission, suspends the
binding, and drains admitted sends. Configure initializes empty schema-three state,
migrates supported schema-one or schema-two state, or checks compatible state.
It prepares a disabled definition for the exact selected release when an
existing binding or explicit schedule setting requires one.

Activation restores captured or explicit enabled intent after the run's
maintenance hold is released. Omitted `daily_email_enabled` preserves intent; an absent binding
stays absent. Explicit true grants standing authority for the
`clew.digest.email` feature's exact daily content. Explicit false prepares
a disabled selection. Existing schedule policy, disabled intent, and failure
halts remain intact. No lifecycle operation approves an incident.

Schema-three ledger reads and short writes can continue during email maintenance.
A scheduled send deliberately skipped under deployment maintenance returns
success. Coordinated recovery keeps email admission held until program and
schedule selections are coherent. Unresolved or foreign holds remain stop
conditions; do not remove them to force activation.

Clockwork owns registration, activation, and failure halts under
`clockwork.schedule.operate`. Clew owns its release-pinned definition and email
runner. Definition generation does not register, activate, or send. The
`clew.digest.email` feature owns the definition's timing and limits.

## Ledger migration

Only coordinated deployment migrates supported schema-one or schema-two ledgers
to schema three. Configure requires the sole run-owned email maintenance hold
and drained sends. Migration excludes concurrent ledger writers.

Schema-two conversion is local. It moves canonical Cast job identity into the
external reference model and marks existing job reports as explicit application
reports. It preserves legacy aliases and exact write namespace. It creates no
threads and requires no Cast or Platter read.

Schema-one migration also requires Platter's public opportunity reader to map
every retained legacy reference to its exact Cast job ID. An empty legacy ledger
needs no Platter read. Missing mappings or two legacy references that select the
same Cast job stop migration. Clew does not infer mappings from company, role, or
URL and does not merge histories. Platter owns the mappings; Clew owns their
application to its ledger.

Migration creates a private consistent SQLite backup of the original schema
under the state root as `ledger-schemaN-backup-RUN_ID-UUID.sqlite3`, with mode
0600. `N` is the original version, one or two. Migration refuses to overwrite an
earlier backup. An interrupted retry creates a new backup. It commits the new
schema, normalized application associations, retained aliases, and exact write
requests in one transaction. Old writers are rejected after commit. Preserve
the backup and reported recovery evidence until deployment completes.

Migration preserves entry IDs, sequence, timestamps, supplied text, correction
links, and retractions. Legacy aliases retain their original argument namespace
for retries. Existing application history remains explicit application history;
no report or thread is invented. Frozen email bytes, occurrences, send keys,
and receipts remain unchanged. Migration starts no collection, preparation,
completion check, or send.

## Backup and recovery compatibility

A consistent filesystem backup requires disabled daily scheduling, stopped Clew
invocations, and finished commands. Preserve both databases, SQLite sidecars, and
maintenance state together in a private backup. Keep the previous complete backup
when restoring compatible history.

Restoring older ledger history changes which write IDs are known. Reconcile
uncertain writes and provider acceptance before replay. Restoring older delivery
history can permit duplicate mail. A ledger backup alone does not justify
restoring older email occurrences.

Only schema-three-compatible programs can operate a migrated ledger. Program recovery does not check ledger compatibility. Select a program that
supports the retained schema. Rollback to an older program requires its matching original-schema
ledger backup, stopped commands and scheduling, and reconciliation of every
post-migration entry and uncertain send. Preserve delivery state.

Older Clew releases do not understand daily email state. Disable the daily binding
and settle admitted sends before explicit rollback to such a release. Compatible
schema-three commands can finish across program selection. An incompatible
migration requires its guarded procedure that excludes writers and preserves a
compatible backup. No completion-time or future compatibility window is promised.

After installation, `clew --register-usage` records command identities in Chancery
without adding reports. CLI usage observation records metadata, not private
content; observation errors do not change product results. Clew installation
remains usable without the Chancery reader.

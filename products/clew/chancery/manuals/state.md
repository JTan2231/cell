# Private state and installation lifecycle

Program publication copies the selected release into fixed regular files beneath
~/Library/Application Support/Clew/install/runtime. Public commands use that runtime
tree; current and previous retain immutable UUID archive selections. Code signing and
runtime path identity are separate from release identity.

New Clockwork definitions use schema 3: they retain the archive release ID, root and
exact hashes, and execute the fixed runtime image. Publication precedes registration.

Before publication, deployment runs the disable transition for owned Clockwork bindings
and waits for their active processes, including an active manual run on a disabled
binding. It restores saved enabled intent after registration; a failed instruction can
leave the owned bindings disabled.

Existing history, delivery records, enabled intent and incident halts retain their
meaning. Retained definitions and wrapper bytes from before this change keep their
legacy execution paths until a new installation or definition selects the runtime image.

Direct install and recovery require clew/daily-email disabled. They reject an enabled
binding and run its disable transition to wait for active processes before runtime
publication. They do not select a new definition; select the matching definition before
enabling a recovered or updated program.

Clew owns its private ledger, retained email occurrences, program selection,
and email admission maintenance. This feature explains their lifecycle and
compatibility. Use `chancery show clew.install.operate` for ordered installation,
migration, schedule preparation, and program recovery procedures.

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
| Immutable releases | `~/Library/Application Support/Clew/install/releases/ID` |
| Public commands | `~/.local/bin/clew`, `~/.local/bin/clew-install` |
| Provider selector | `~/Library/Application Support/Chancery/providers/clew` |

Global `--state-dir ABSOLUTE_PATH` selects an independent private ledger for
ordinary Clew commands. Deployment configures only the default ledger. The state
directory has mode 0700; the databases are regular files with mode 0600. Keep
notes, statuses, thread names, references, and frozen messages private.

The `clew.ledger.use` feature owns general entry meaning, named threads,
external references, append transactions, exact retry identity, and corrections.
The `clew.application.track` feature owns explicit application reports and the
job-specific view. Plain external job links do not create application reports.

The `clew.digest.email` feature owns message bytes, occurrences, receipts,
submission recovery, and email schema-one behavior. These records have no
automatic pruning. Clew supplies no data backup, data restore or deletion operation. Existing
backup files remain unchanged.

## Initialization and observations

`clew init` and `clew doctor` print readable results by default. Use `--json`
for the schema-four machine response. Human errors go to stderr. JSON errors
retain `ok: false` and `error.detail` on stdout, with a nonzero exit.

Init creates ledger schema four in an empty database. It can finish initialization
when an interruption left an empty database. It preserves compatible existing
rows and refuses nonempty foreign or unsupported state. Ordinary commands never
initialize or migrate state implicitly. Init and ordinary commands refuse
schema-one and schema-two ledgers.

Doctor checks the local schema, SQLite integrity, and correction references. It
returns the retained entry count without adding entries or probing Milieu.
Installation inspection describes selected release metadata. It does not
check artifact integrity or establish readiness.

Coordinated deployment creates or migrates state and selects program files. It
does not invoke doctor, check release integrity, or probe the Milieu snapshot.

## Program and provider selection

A release contains `clew`, `clew-install`, the recovery installer, and the complete
matching Chancery bundle. An opaque UUID identifies the immutable release;
package version, feature contract version, and database schema are separate
identities. Provider pages and overview participate in release identity and
release selection.

The shared `cell-install-v4` helper selects public commands and the Clew provider
with atomic replacements of individual selectors. Failed publication retains
completed changes. Foreign selectors and stale expected selections stop
publication. The installer accepts `--home ABSOLUTE_PATH` and
`--expected-current absent|releases/ID`. Direct installation selects program
files only. It does not initialize or migrate state.

Program recovery selects a retained `cell-install-v4` release and
preserves separate ledger and email state. It does not restore, delete, or rewrite
rows, retry uncertain mail, or approve a scheduling incident. No older
installation format or automatic schema downgrade is supported. Preserve
unresolved installation evidence when recovery fails.

## Product deployment and email maintenance

The deployment command captures `clew/daily-email`, selects program files, and
enters ordinary admission. It initializes empty schema-four state, migrates
supported schema-one, schema-two, or schema-three state, or opens compatible state. It then
retargets and selects the owned daily definition directly. Deployment creates no
hold, waits for owned scheduled activations before runtime publication, and preserves prior enabled intent.

Omitted `daily_email_enabled` preserves saved intent; an absent binding stays
absent. Explicit true grants standing authority for the `clew.digest.email`
feature's exact daily content. Explicit false selects disabled intent. Existing
schedule policy and failure halts remain intact. No deployment step approves an
incident.

Explicit email maintenance retains its ordinary owner and admission rules.
Schema-four ledger reads and short writes can continue during that maintenance.
A scheduled send deliberately skipped under maintenance returns success. An
existing hold can refuse ordinary deployment setup; deployment does not release
it. Failed instructions retain completed effects without automatic retry,
rollback, or product recovery. Inspect current product state before a new attempt.

Clockwork owns registration, activation, and failure halts under
`clockwork.schedule.operate`. Clew owns its release-pinned definition and email
runner. Definition generation does not register, activate, or send. The
`clew.digest.email` feature owns the definition's timing and limits.

## Ledger migration

The product deployment command migrates supported schema-one, schema-two, or schema-three
ledgers to schema four under ordinary admission. Migration excludes concurrent
ledger writers. It does not create a maintenance hold or drain email.

Schema-two conversion is local. It moves canonical Milieu job identity into the
external reference model and marks existing job reports as explicit application
reports. It preserves legacy aliases and exact write namespace. It creates no
threads and requires no Milieu or Platter read.

Schema-three conversion renames the owned job column, reference namespace, and
write-request field to Milieu in one transaction. Exact retries use the renamed
request form with the original write ID. Supplied text, opaque job IDs, entry
order, corrections, and frozen email records stay unchanged. No dependency read
is required. Older programs refuse schema four.

Schema-one migration also requires Platter's public opportunity reader to map
every retained legacy reference to its exact Milieu job ID. An empty legacy ledger
needs no Platter read. Missing mappings or two legacy references that select the
same Milieu job stop migration. Clew does not infer mappings from company, role, or
URL and does not merge histories. Platter owns the mappings; Clew owns their
application to its ledger.

Migration commits the new schema, normalized application associations,
retained aliases, and exact write requests in one transaction. It creates no
copy of the original database. A failure before commit retains the prior state.
Old writers are rejected after commit. Preserve reported transaction evidence
until deployment completes.

Migration preserves entry IDs, sequence, timestamps, supplied text, correction
links, and retractions. Legacy aliases retain their original argument namespace
for retries. Existing application history remains explicit application history;
no report or thread is invented. Frozen email bytes, occurrences, send keys,
and receipts remain unchanged. Migration starts no collection, preparation,
completion check, or send.

## Program recovery compatibility

Only schema-four-compatible programs can operate a migrated ledger. Program
recovery preserves ledger and email state and never restores earlier history.
Select a program that supports the retained schema. No schema downgrade is
supplied. Reconcile uncertain writes and provider acceptance before replay.

Older Clew releases do not understand daily email state. Disable the daily
binding and settle admitted sends before selecting such a release. Compatible
schema-four commands can finish across program selection. Migration excludes
writers and commits atomically. No completion-time or future compatibility
window is promised.

After installation, `clew --register-usage` records command identities in Chancery
without adding reports. CLI usage observation records metadata, not private
content; observation errors do not change product results. Clew installation
remains usable without the Chancery reader.

# Operate the Todo installation and database

Todo is a synchronous local CLI with a SQLite database. It has no Todo daemon.
The user installation additionally owns one daily email LaunchAgent; Nucleus
is a separate service used only by model-assisted research.

## Database initialization and migration

Create a new database without replacing anything:

```sh
/Users/joey/.local/bin/todo --database <DATABASE_PATH> init
```

Ordinary commands never migrate implicitly. For a version-one database, stop
callers and choose an absolute backup path that does not exist:

```sh
/Users/joey/.local/bin/todo --database <DATABASE_PATH> migrate \
  --backup <ABSENT_ABSOLUTE_BACKUP_PATH>
```

Todo creates and retains a complete SQLite backup before the version-two
transaction. Failure leaves the original usable. Against a current database,
migrate is a no-op and does not touch the supplied backup path.

Migration preserves identities, lifecycle, sources, notes and historical
direction in their documented destination records.

## Deploy or update on macOS

Build and test a candidate, verify Nucleus, and invoke the product deployer
only with explicit installed-state authority:

```sh
cd /Users/joey/rust/cell
./todo/ci.sh
<TESTED_TODO_INSTALL> install \
  --binary <ABSOLUTE_TODO_BINARY> \
  --bundle /Users/joey/rust/cell/todo/chancery \
  --package /Users/joey/rust/cell/todo/packaging/macos \
  --email-from <SENDER> \
  --email-to <RECIPIENT>
```

A fresh install requires both email-address flags. An update may preserve
existing complete email configuration by omitting both; supplying only one is
an error.

The Rust `todo-install` executable is sealed with the tested Todo binary.
It uses shared `cell-install-v2` immutable file inventories and selector
transactions, while Todo owns admission, database migration and schedule
recovery. The predecessor format-1 release remains verifiable. The installed
frontend is Rust; the static zsh email runner retains its credential contract.

The deployer stages a content-addressed release, records the email LaunchAgent's
loaded state and stops it. It creates a private transaction directory, runs
the candidate's migration with a backup, then switches the release selector.
It validates the installed CLI before installing the final plist. Updates reload the
schedule only if it was loaded before the update; unloaded schedules remain
unloaded, and launchd enable/disable overrides are never changed. A fresh install
loads its schedule only if no existing plist or disabled override records an
operator choice. Loaded-and-disabled services are refused before quiescence
because restoring them would require changing the disabled override. A
pre-commit failure restores the captured database, release, frontend,
configuration, plist, and loaded-service state. The previous release remains
available through the installation selector.
The coordinated adapter captures loaded/disabled state and checks the owned
rendered plist against the selected release. An ambiguous service-state change
keeps the deployment hold during verification or recovery; it is not silently
reloaded or reported recovered.

Standalone installation creates its own durable admission hold; coordinated
installation uses the captured run's hold. Recovery restores a migration backup
through SQLite's exclusive destination locking while public commands remain
suspended. Failed recovery retains maintenance and private transaction data;
it never treats matching program files as proof of database recovery.

Nucleus authentication and state are never part of Todo rollback. The Resend
key remains in the process environment rather than Todo config or plist.

## Validate operation

```sh
/Users/joey/.local/bin/nucleus health
/Users/joey/.local/bin/todo email preview
/Users/joey/.local/bin/todo email send
launchctl print "gui/$(id -u)/org.todo.daily-email"
```

Email send is externally consequential and requires separate authorization;
preview can validate content without disclosure. The LaunchAgent runs at local
09:00 without RunAtLoad and may run after wake. It cannot submit while the Mac
is off or the user is logged out.

Todo database, backups, config, logs, and email content are private. Do not
delete retained state, overwrite backups, publish a release, or change Resend
account state merely as part of installation diagnosis.

## Coordinated deployment maintenance

```sh
todo --json maintenance hold RUN_ID
todo --json maintenance status
todo --json maintenance ready RUN_ID
todo --json maintenance release RUN_ID
```

Normal database and config selection applies. The selected database's parent
directory contains `deployment-maintenance/`. Databases in that directory share the gate.
New research and ordinary mutations, including scheduled email send, retain a
shared admission guard through completion. Holds prevent new admissions
before input is retained, and do not interrupt already admitted research or
change an operator configuration. Read-only commands remain available.

JSON returns `data.maintenance` with `protocol_version: 1`, `holds`, `drained`,
and `nonterminal_jobs`. Drain requires both no live admission guard and no
accepted/running/waiting Nucleus jobs attributed to Todo, conservatively across
all Todo databases. This catches a killed CLI whose job still exists. An
unavailable runtime returns a null count and `drained: false`; it is never
reported as zero. Holds survive process exit and release removes only its
exact owner.

`migrate --backup` with `CELL_DEPLOYMENT_RUN_ID` requires its sole matching
hold and exclusive activity; no caller-supplied value bypasses another hold.
The macOS deployer accepts `--expected-current absent|releases/HASH` under its
update lock, preserves configured email addresses, and accepts either ordinary
strict Nucleus readiness or the named deployment's proved held readiness.

Deployment verification checks the installed release, configured database,
Nucleus readiness, maintenance settlement, and captured email-service state.
It creates no concern, routing proposal, or model job and sends no email.
Ordinary Todo success remains its committed domain result after a later
runtime failure.

`maintenance ready RUN_ID` requires the sole drained hold. It checks whether
this binary can read the configured database: version, required table, index
and trigger definitions, SQLite integrity, and foreign keys. Use it to check
storage compatibility after an interrupted migration.

Deployment admission resolves the configured database to its canonical path and
uses that database parent for `deployment-maintenance/`. Symbolic aliases share
the same gate. Databases with multiple hard links are rejected because their
state root cannot identify one authoritative admission gate.

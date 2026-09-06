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
migrate is a true no-op and does not touch the supplied backup path.

Migration preserves identities, lifecycle, sources, notes, and historical
direction. It does not infer cross-todo relationships, assessment facts,
accepted design, implementation, or closure evidence.

## Deploy or update on macOS

Build and test a candidate, verify Nucleus, and invoke the product deployer
only with explicit installed-state authority:

```sh
cd /Users/joey/rust/cell
./todo/ci.sh
./todo/packaging/macos/deploy-user.sh \
  --binary <ABSOLUTE_TODO_BINARY> \
  --email-from <SENDER> \
  --email-to <RECIPIENT>
```

A fresh install requires both email-address flags. An update may preserve
existing complete email configuration by omitting both; supplying only one is
an error.

The deployer stages a content-addressed release, records whether the email
LaunchAgent is loaded, quiesces it, creates a private transaction directory,
runs the candidate's backup-bearing migration, switches the release selector,
validates the installed CLI, then installs the final plist. Updates reload the
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
reloaded or reported recovered. Its isolated canary never sends email.

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
todo --json maintenance canary --directory /absolute/private/canary-directory
```

Normal database/config selection applies. The selected database parent owns
`deployment-maintenance/`; databases sharing a parent share the same gate.
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

The canary uses a product-marked private synthetic database and source file,
runs actual concern-routing research, and proves one pending routing record
and the correlated terminal Todo Nucleus job. It returns `data.canary` with
`protocol_version`, `verified`, database, concern, routing, and job identities.
It never accepts a proposal or sends email. Foreign directories are refused.
If interrupted research has no domain result, it fails with retained evidence
instead of creating a replacement attempt. Requester canaries run after
Nucleus admission is restored, while production Todo holds remain.

`maintenance ready RUN_ID` requires the sole drained hold and proves that this
binary can read the actual configured database: current version, every required
table/index/trigger definition, SQLite integrity, and foreign keys. This is the
production storage compatibility proof after an interrupted migration; the
isolated canary alone cannot supply it.

Deployment admission resolves the configured database to its canonical path and
uses that database parent for `deployment-maintenance/`. Symbolic aliases share
the same gate. Databases with multiple hard links are rejected because their
state root cannot identify one authoritative admission gate.

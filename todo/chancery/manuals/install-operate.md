# Operate the Todo installation and database

Todo is a synchronous local CLI with a SQLite database. It has no Todo daemon.
Todo declares the `todo/daily-email` Clockwork binding; Clockwork owns its
generated LaunchAgent, failure admission and incident notification. Nucleus
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

Build and test a candidate, verify compatible Nucleus and Clockwork, and invoke
the product deployer only with explicit installed-state authority:

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
frontend is Rust. The pinned native daily runner executes its exact release's
packaged zsh credential script and `libexec/todo` payload, rather than a mutable
public selector. The script sources `~/.zshrc`, passes only the Resend key through
its existing scrubbed environment, and never places it in process arguments.

The deployer stages a content-addressed release and captures the exact prior
Clockwork binding plus any legacy `org.todo.daily-email` plist, loaded state
and disabled override. It proves ownership against the installed release and
refuses competing active schedules or a loaded legacy service whose disabled
override prevents safe restoration.

Under its durable maintenance hold, it drains admissions, disables an enabled
Clockwork binding and boots out a loaded legacy service. It records private
recovery evidence and runs the candidate's migration with a backup. After held
storage readiness, it removes only the verified legacy plist, registers an
inert schema-two definition, publishes and validates selectors, then selects
the Clockwork definition. The native runner, config and private log destinations
remain Todo-owned. Clockwork owns the generated LaunchAgent and runtime state.

Updates preserve enabled or disabled intent. A fresh installation enables only
when no prior binding, legacy plist or disabled override records an operator
choice. The installer does not change legacy launchd enable/disable overrides.
The previous release remains available. Recovery disables the candidate before
restoring the exact prior binding or legacy projection; it never intentionally
loads both. Unattributable drift retains the hold and recovery evidence.

The schema-two definition declares `halt-until-approved`. A Clockwork failure
incident survives binding changes, deployment, rollback and maintenance release.
Only explicit continuation of that exact incident permits future scheduling.

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
clockwork binding show todo/daily-email
clockwork history todo/daily-email --limit 20
clockwork incident list todo/daily-email
```

Email send is externally consequential and requires separate authorization;
preview can validate content without disclosure. The definition runs at local
09:00 without run-at-load, skips overlap and limits activation to 180 seconds.
It depends on the macOS GUI session and may run after wake; it cannot promise
submission while the Mac is off or the user is logged out.

Startup failure, crash, timeout or nonzero send completion halts scheduling in
Clockwork. Inspect `clockwork incident show INCIDENT_ID`. After resolving the
cause and any uncertain Resend acceptance, use `clockwork binding resume
todo/daily-email INCIDENT_ID` for explicit continuation. It does not send or
retry a digest. Clockwork owns the retained incident and sends its notification
through Email's installed CLI to Email's fixed personal recipient. Todo's digest
continues to use direct Resend transport and Todo-configured addresses.

Todo database, backups, config, logs, and email content are private. Do not
delete retained state, overwrite backups, publish a release, or change Resend
account state merely as part of installation diagnosis.

## Coordinated deployment maintenance

The coordinator's `apply` phase stages and verifies immutable release files.
`configure` runs the product-owned configuration, migration and selector
transaction with its schedule disabled. `verify` checks the installed result
without starting product work. `release` removes only the named admission hold.
After every affected hold is released, `activate` restores the captured enabled
state of the current selected definition. An originally disabled binding stays
disabled. Clockwork incident halts and product pauses remain in force.

Drain returns `waiting` while admitted commands or durable Nucleus jobs remain.
It neither cancels nor retries those jobs. A completely absent Nucleus
installation with no Nucleus database has no durable jobs to drain. An
unavailable existing runtime is not treated as an empty job inventory.

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
change an operator configuration. A scheduled digest refused only by a hold
returns success with `data` equal to
`{"scheduled":true,"skipped":"deployment_maintenance"}`. It sends nothing and
does not create a failure incident. Other admission errors remain failures.
Read-only commands remain available.

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

For a fresh coordinated installation, provide `email_from` and `email_to` in
Todo's deployment settings. An update preserves complete existing addresses
when both are omitted. Configuration must contain both nonblank addresses.
The adapter records the product transaction before it suspends public access.
Recovery checks the same owner and candidate, restores a migration backup for
a pre-commit failure, or proves the committed candidate's storage readiness.
It keeps the selected schedule disabled until group activation. Recovery
artifacts remain private under `backups/deployments/`.

Deployment settings accept only `email_from`, `email_to`, and `enabled`.
Addresses must be strings and `enabled` must be a boolean. For example,
`{"todo":{"enabled":false}}` keeps an updated candidate schedule disabled
after group activation. Fresh installation also requires both email addresses.
An omitted `enabled` preserves captured intent; a new schedule defaults to
enabled. Recovery to the prior configuration ignores this override.

When recovery restores an owned legacy LaunchAgent, it leaves that service
unloaded while admission is held. Final activation restores its captured load
state only after checking the original release, exact plist digest, complete
schedule definition, and absence of an enabled Clockwork binding. An altered
legacy plist retains the recovery failure instead of loading a foreign service.

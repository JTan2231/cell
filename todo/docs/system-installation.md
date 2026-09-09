# User-owned macOS installation

Todo runs synchronously. It owns no daemon, root-owned files or log service.
Its user installation declares the `todo/daily-email` Clockwork binding.
Clockwork owns scheduled admission, failure halts and its generated LaunchAgent.
Routing, assessment, design and `todo new` research use the user's separately
installed Nucleus service for Codex execution and authentication. Reads,
decisions, migration and email delivery do not use Nucleus.

## Deploy

Install and authenticate Nucleus first, verify the email sender domain in
Resend, and place the API key in the installed user's `~/.zshrc`:

```sh
export RESEND_API_KEY='re_replace_with_the_real_key'
```

The key is not stored in Todo's configuration or LaunchAgent plist. The
pinned native `bin/todo-daily-email` runner invokes its exact release's packaged
zsh script and `libexec/todo` payload. The script sources `~/.zshrc` for each
occurrence, extracts the key, and starts Todo with a scrubbed environment.
It does not follow the mutable public Todo selector. The secret is absent from the
plist and process arguments.

Then build and test Todo and pass its absolute executable path plus the
deployment's sender and recipient to the deployer:

```sh
cd /Users/joey/rust/cell/todo
./ci.sh
<TESTED_TODO_INSTALL> install \
  --binary <TESTED_TODO_BINARY> \
  --bundle /Users/joey/rust/cell/todo/chancery \
  --package /Users/joey/rust/cell/todo/packaging/macos \
  --email-from 'todo@joeytan.dev' \
  --email-to 'j.tan2231@gmail.com'
```

These addresses configure this deployment; they are not Todo product defaults.
`todo@joeytan.dev` must belong to the domain verified in Resend.

The layout is:

```text
~/.local/bin/todo
~/Library/LaunchAgents/org.clockwork.todo.daily-email.plist  # Clockwork-owned
~/Library/Application Support/Chancery/providers/
  todo -> Todo's current release share/chancery/todo
~/Library/Application Support/Todo/
  config.toml
  todo.db
  install/
    releases/<content-hash>/
      bin/todo
      bin/todo-install
      bin/todo-daily-email
      libexec/todo
      package/install
      package/org.todo.daily-email.plist
      share/chancery/todo/
        provider.json
        entries/
        manuals/
      manifest.json
    current -> releases/<content-hash>
    previous -> releases/<content-hash>
~/Library/Logs/Todo/
  email.stdout.log
  email.stderr.log
```

`todo-install` is a Rust executable built and sealed beside the tested Todo
binary. It uses shared `cell-install-v2` exact immutable inventories and
selector transactions. Todo still owns database migration, admission and
schedule recovery. The former format-1 package remains verifiable during
migration. `todo-install inspect` and `verify-release ABSOLUTE_PATH` are
read-only and never send email or execute a retained release.

If no database or config selector is explicit, the installed Rust frontend at
`~/.local/bin/todo` selects `config.toml`. The config points at `todo.db` and selects high liaison
quality. Its `[email]` section contains the deployment-specific `from` and `to`
values. Nucleus is resolved through `NUCLEUS_SOCKET` when set, or its standard
per-user socket otherwise.

Deployment first verifies the installed Nucleus service is healthy and stages a
complete content-addressed release, including Todo's Chancery provider bundle.
The bundle hash participates in the release identity and integrity manifest.
The Todo-owned `providers/todo` selector points through Todo's atomic `current`
release selector, so documentation and executable rollback together. Todo
creates that one selector even when Chancery is not yet installed, never changes
another provider's selector, and does not depend on Chancery at runtime.

Before changing database state, the installer captures the prior Clockwork
binding and any supported legacy `org.todo.daily-email` plist, loaded state and
disabled override. It proves that these records belong to the selected Todo
release. Competing enabled Clockwork and legacy schedules are refused. Loaded
legacy services with a disabled override or no attributable plist are refused;
restoring them would require changing operator state.

Standalone installation creates its own durable admission hold. Coordinated
deployment uses the captured run's hold. The installer drains admitted work,
disables an enabled Clockwork binding and boots out a loaded legacy service.
It records private recovery evidence, runs the candidate's backup-bearing
migration, and proves held storage readiness before publishing selectors.

Under that hold, the installer removes only the verified legacy plist and
registers an inert schema-two Clockwork definition for the exact native runner.
It then publishes and checks the installed CLI and selects the definition.
An update restores captured enabled intent; an inactive schedule stays inactive.
A fresh installation enables its binding only when no prior binding, legacy
plist or disabled override records an operator choice. The installer never
changes legacy launchd enable/disable overrides.

Clockwork's failure halt is independent of enabled intent. Definition changes,
installation, rollback and maintenance release do not clear an incident. The
new definition has no run-at-load trigger. A scheduled send that meets a
maintenance hold returns a successful skip and does not send mail.

A failed update restores captured database, configuration, selectors and exact
prior schedule state. Recovery first disables the candidate binding and any
active legacy service, then restores the verified prior binding or legacy
projection. It never intentionally loads both. Database restoration uses
SQLite's exclusive destination locking while public mutation remains suspended.
Nucleus authentication and state are never restored by Todo.

If ownership, exclusive database recovery or schedule restoration cannot be
proved, the installer retains maintenance and private transaction evidence.
Do not infer recovery from matching program files alone. Clockwork retains its
own definitions and incident history independently of Todo's transaction files.
An update retains the prior release through `previous`. The transaction-local
migration backup is removed only after success; it is not a user backup policy.

Running the same deploy command with a new release binary performs an update.
An identical package reuses its release directory. A fresh install requires
`--email-from` and `--email-to` together. On an update, omitting both preserves
the existing `config.toml` byte-for-byte when it already has an `[email]`
section; an old config without that section requires both flags once. Providing
both updates the email section while preserving the other configured settings, and
supplying only one is an error.

## Manual database migration

Ordinary commands never migrate a database implicitly. To upgrade a database
outside the deployer, first stop callers and choose an absolute backup path
that does not exist:

```sh
clockwork binding disable todo/daily-email
todo --database "/absolute/path/to/todo.db" migrate \
  --backup "/absolute/path/to/retained/todo-v1.db.backup"
```

For a version-1 database, Todo creates the complete backup and retains it after
a successful transactional migration. It refuses a relative or existing
backup path. Verify the migrated database before restoring the captured Clockwork selection
and enabled intent. Do not clear a failure incident as part of migration.
When the selected database is already current, `migrate` succeeds as a no-op
without creating, reading, or modifying the supplied backup path.

## Schedule and validation

The product-owned schema-two definition uses machine-local 09:00, no
run-at-load, skipped overlap and a 180-second activation timeout. It pins the
exact native `bin/todo-daily-email` image with `HOME` as its only registered
environment value. Clockwork verifies that image and invokes it directly.
The runner executes its same-release script and `libexec/todo` payload with
`email send --scheduled`. The digest sends immediately with a stable key for
the most recent local 09:00 occurrence; it does not submit Resend `scheduled_at`.

The definition declares the default `halt-until-approved` policy. Startup
failure, nonzero exit, crash or timeout closes Clockwork admission and retains
one incident notification. Clockwork uses the installed Email CLI to submit
that alert to Email's fixed personal recipient. Todo continues to send the
digest directly through Resend to its separately configured recipient.
Credentials and digest content do not enter the Clockwork definition or alert.

Inspect without sending:

```sh
todo email preview
clockwork binding show todo/daily-email
clockwork history todo/daily-email --limit 20
clockwork incident list todo/daily-email
clockwork incident show INCIDENT_ID
```

After resolving the cause and checking any uncertain Resend outcome, explicitly
approve future scheduling:

```sh
clockwork binding resume todo/daily-email INCIDENT_ID
```

Resume requires the exact incident ID. It does not send, replay a missed
occurrence, reconcile provider acceptance, or extend Resend deduplication.
`todo email send` is a separate authorized immediate send with its own ad hoc
key. Todo retains no delivery table, frozen cross-invocation digest, or
background retry queue. One invocation retries only its frozen payload/key
within the existing three-attempt transport bound.

Product stdout and stderr remain in `~/Library/Logs/Todo/email.stdout.log` and
`email.stderr.log`. Clockwork owns its separate broker logs and incident state.
The timer depends on the user's macOS GUI session and launchd delivery. A
sleeping Mac can run after wake; a powered-off Mac or logged-out user has no
09:00 execution guarantee. Neither Clockwork history nor an accepted Resend
receipt proves final inbox delivery.

The digest sends aggregate counts, every open canonical todo's current title,
generic plain-language stage labels, typed concern, routing, todo, assessment,
and design references when applicable, and read-only inspection commands to
Resend and the recipient's email provider. It excludes concern bodies,
directions, notes, source paths, assessment and design summaries,
unresolved-choice text, and evidence. Resend documents
[30-day retention](https://resend.com/docs/knowledge-base/account-quotas-and-limits)
of email content and metadata. Treat the digest as an external disclosure of
the included Todo state and avoid the feature if that retention is
inappropriate.

## Explicit targets

The installed frontend's default is only a convenience. These continue to
bypass it:

```sh
todo --database /path/to/other.db list
TODO_DATABASE=/path/to/other.db todo list
todo --config /path/to/other.toml list
TODO_CONFIG=/path/to/other.toml todo list
```

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

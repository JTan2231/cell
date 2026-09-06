# User-owned macOS installation

Todo is synchronous and owns no daemon, root-owned files, or log service. Its
user installation includes a LaunchAgent that runs the synchronous email
command once per day. Routing, assessment, design, and `todo new` research use
the same user's separately installed Nucleus service, which owns Codex
execution and authentication; deterministic reads, decisions, migration, and
email delivery do not use Nucleus.

## Deploy

Install and authenticate Nucleus first, verify the email sender domain in
Resend, and place the API key in the installed user's `~/.zshrc`:

```sh
export RESEND_API_KEY='re_replace_with_the_real_key'
```

The key is not stored in Todo's configuration or LaunchAgent plist. The
packaged zsh runner sources `~/.zshrc` for each occurrence, extracts the key,
and starts Todo with a scrubbed environment. The secret is absent from the
plist and process arguments.

Then build and test Todo and pass its absolute executable path plus the
deployment's sender and recipient to the deployer:

```sh
cd /Users/joey/rust/cell/todo
./ci.sh
./packaging/macos/deploy-user.sh \
  --binary "/Users/joey/rust/cell/target/release/todo" \
  --email-from 'todo@joeytan.dev' \
  --email-to 'j.tan2231@gmail.com'
```

These addresses configure this deployment; they are not Todo product defaults.
`todo@joeytan.dev` must belong to the domain verified in Resend.

The layout is:

```text
~/.local/bin/todo
~/Library/LaunchAgents/org.todo.daily-email.plist
~/Library/Application Support/Chancery/providers/
  todo -> Todo's current release share/chancery/todo
~/Library/Application Support/Todo/
  config.toml
  todo.db
  install/
    releases/<content-hash>/
      bin/todo
      bin/todo-daily-email
      libexec/todo
      package/todo
      package/todo-daily-email
      package/deploy-user.sh
      package/org.todo.daily-email.plist
      share/chancery/todo/
        provider.json
        entries/
        manuals/
      manifest.txt
    current -> releases/<content-hash>
    previous -> releases/<content-hash>
~/Library/Logs/Todo/
  email.stdout.log
  email.stderr.log
```

`~/.local/bin/todo` selects `config.toml` when no explicit database or config
selector is present. The config points at `todo.db` and selects high liaison
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
Todo CI validates the bundle and requires its declared provider release to equal
the Todo package version; `release.sh` bumps and commits both versions together.

Before an update can change database state,
it records whether the email LaunchAgent is loaded and quiesces it. It creates a
private transaction directory, asks the candidate binary to run `todo migrate
--backup` with a nonexistent absolute path inside that directory, switches the
release selector, and validates the installed CLI. Only then does it install
the final `org.todo.daily-email` definition. It bootstraps an update only if
the schedule was previously loaded; an unloaded schedule remains unloaded.
Fresh installation bootstraps only when no existing plist or disabled override
records an operator choice. The deployer never changes launchd enable/disable
overrides. A service that is both loaded and disabled is refused before
quiescence because launchd cannot reload it without changing that override.
Coordinated deployment retains Todo's maintenance hold throughout these changes
and its isolated canary never sends email.
The Todo deployment adapter captures loaded/disabled state and proves that the
live plist matches the selected release's rendered template. Verification and
recovery retain the hold if those controls drift, including interruption between
bootout and bootstrap. Recovery never guesses whether an unload was an operator
pause or an incomplete installer step.

The candidate migrator writes a complete pre-migration SQLite backup before
the version-2 transaction begins. If migration, selector switching, smoke
testing, plist installation, or bootstrap later fails, the deployer restores
the prior database, release selector, frontend, configuration, plist, and
loaded-service state. An update retains the prior release through `previous`.
The transaction-local backup is removed only after the update has succeeded;
it is not a user backup policy.

Running the same deploy command with a new release binary performs an update.
An identical package reuses its release directory. A fresh install requires
`--email-from` and `--email-to` together. On an update, omitting both preserves
the existing `config.toml` byte-for-byte when it already has an `[email]`
section; an old config without that section requires both flags once. Providing
both regenerates the standard installed config with the supplied values, and
supplying only one is an error.

## Manual database migration

Ordinary commands never migrate a database implicitly. To upgrade a database
outside the deployer, first stop callers and choose an absolute backup path
that does not exist:

```sh
launchctl bootout "gui/$(id -u)/org.todo.daily-email" 2>/dev/null || true
todo --database "/absolute/path/to/todo.db" migrate \
  --backup "/absolute/path/to/retained/todo-v1.db.backup"
```

For a version-1 database, Todo creates the complete backup and retains it after
a successful transactional migration. It refuses a relative or existing
backup path. Verify the migrated database before reloading the LaunchAgent.
When the selected database is already current, `migrate` succeeds as a no-op
without creating, reading, or modifying the supplied backup path.

## Schedule and validation

The LaunchAgent uses launchd `StartCalendarInterval` with hour `9` and minute
`0`. That means 09:00 according to the Mac's local clock, including local
daylight-saving changes. It has no `RunAtLoad`. It invokes
`todo email send --scheduled`, which sends the current digest immediately and
uses a stable key for the most recent local 09:00 occurrence. It does not submit
a future Resend `scheduled_at` request.

If the logged-in Mac is asleep at 09:00, launchd coalesces the occurrence and
runs it after wake. A user LaunchAgent cannot guarantee a 09:00 submission
while the Mac is powered off or the user is logged out; that stricter guarantee
would require an always-on host with access to Todo's authoritative state.

Validate the content and delivery immediately after deployment instead of
waiting for the next 09:00 occurrence:

```sh
source "$HOME/.zshrc"
todo email preview
todo email send
launchctl print "gui/$(id -u)/org.todo.daily-email"
```

The manual send uses its own ad-hoc idempotency key and does not consume the
scheduled occurrence's key. Confirm receipt and inspect Resend's delivery log.
For later launchd failures, read
`~/Library/Logs/Todo/email.stderr.log`; successful command output goes to the
adjacent `email.stdout.log`. There is no Todo delivery table or background
retry queue.

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
and the exact correlated `todo/concern-routing/1` Nucleus job. Deployment
verification separately requires a completed job and matching completed current
attempt with nonblank thread, turn, and final message output. A committed routing
proposal still remains ordinary Todo success after a later runtime failure;
that outcome alone does not verify a healthy deployment. Rechecking a repaired
Nucleus output read reuses the existing canary job and routing record. It returns `data.canary` with
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

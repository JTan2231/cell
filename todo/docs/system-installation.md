# User-owned macOS installation

Todo is synchronous and owns no daemon, root-owned files, or log service. Its
user installation includes a LaunchAgent that runs the synchronous email
command once per day. `todo new` uses the same user's separately installed
Nucleus service, which owns Codex execution and authentication; email delivery
does not use Nucleus.

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

Deployment first verifies the installed Nucleus service is healthy, stages a
complete content-addressed release, writes or preserves configuration, switches
`current`, initializes the database on a fresh install, validates the installed
CLI, installs the LaunchAgent, and bootstraps `org.todo.daily-email`. An update
retains the prior release through `previous`. If installation or service
validation fails, the deployer restores the prior release selector, frontend,
configuration, plist, and loaded-service state. It never deletes the user's
existing database.

Running the same deploy command with a new release binary performs an update.
An identical package reuses its release directory. A fresh install requires
`--email-from` and `--email-to` together. On an update, omitting both preserves
the existing `config.toml` byte-for-byte when it already has an `[email]`
section; an old config without that section requires both flags once. Providing
both regenerates the standard installed config with the supplied values, and
supplying only one is an error.

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

The digest sends todo titles to Resend and the recipient's email provider.
Resend documents
[30-day retention](https://resend.com/docs/knowledge-base/account-quotas-and-limits)
of email content and metadata. Treat the digest as an external disclosure of
every open todo title and avoid the feature if that retention is inappropriate.

## Explicit targets

The installed frontend's default is only a convenience. These continue to
bypass it:

```sh
todo --database /path/to/other.db list
TODO_DATABASE=/path/to/other.db todo list
todo --config /path/to/other.toml list
TODO_CONFIG=/path/to/other.toml todo list
```

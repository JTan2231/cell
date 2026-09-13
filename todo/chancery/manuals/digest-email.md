# Preview or send the Todo daily attention digest

The digest reads unresolved concerns and open canonical todos. It groups them
by the attention they need:

- **Needs your decision** includes a routing proposal, situation choice, or
  desired-state design awaiting an explicit user decision.
- **Needs follow-up** includes an unresolved concern without a pending routing
  decision, or an open todo awaiting assessment, named assessment inputs,
  reassessment after changed bases, or desired-state design work.
- **Other open todos** includes the remaining open todos. Accepting a desired
  state preserves the umbrella's open lifecycle.

Empty sections are omitted. Every item leads with a current title or
plain-language label and a plain-language status. A secondary `Reference:`
line spells out typed references such as `Concern cN`, `Routing proposal rN`,
`Todo tN`, `Situation assessment aN`, and `Desired-state design dN`. `Inspect:`
lines contain read-only CLI commands. Stored state tokens are
never the user-facing status, and the email contains no decision commands.

The subject combines the number of items needing attention with the number of
open todos, for example `Todo daily: 3 need attention · 6 open todos`. The body
summary also reports the unresolved-concern count. If there are no open todos
or unresolved captured concerns, the subject is `Todo daily: all clear` and
the body says that nothing needs attention.

## Preview first

```sh
/Users/joey/.local/bin/todo email preview
```

Preview renders the exact configured message without reading
`RESEND_API_KEY` or making a network request. Human output includes From, To,
Subject, and plain-text body. JSON also exposes HTML plus `attention_count`,
`pending_concern_count`, and `todo_count`; `todo_count` remains the number of
open canonical todos. Use preview when validating content or disclosure before
any external action.

## Send

```sh
/Users/joey/.local/bin/todo email send
```

Send requires `[email]` configuration and `RESEND_API_KEY` in the process
environment. The key must be nonblank with no surrounding whitespace. Send freezes one body and
`todo-email/<UUIDv7>` idempotency key for up to three attempts on transport,
rate-limit, or server failures. It sends immediately; Todo has no delivery
database or background retry queue.

The `todo/daily-email` Clockwork definition pins the installed native runner.
That runner executes its same-release credential script and Todo payload.
The corresponding explicit CLI operation is:

```sh
/Users/joey/.local/bin/todo email send --scheduled
```

Scheduled mode uses `todo-daily-email/<LOCAL YYYY-MM-DD>` for the most recent
local 09:00 occurrence. It does not submit a Resend `scheduled_at` value. A
manual send uses a different key and does not consume the scheduled
occurrence. Clockwork uses local 09:00, no run-at-load, skipped overlap and a
180-second activation limit. The product-owned schema-two definition declares
`halt-until-approved`.

A startup failure, crash, timeout or nonzero send outcome closes Clockwork
admission and retains one incident email. Inspect `clockwork incident list
todo/daily-email` and `clockwork incident show INCIDENT_ID`. After resolving the
cause, explicitly approve future scheduling with `clockwork binding resume
todo/daily-email INCIDENT_ID`. Enable, release changes and maintenance do not
clear the halt. Continuation starts no send and does not reconcile acceptance.

Inspect Resend before retrying an uncertain submission. Todo retains no frozen
digest across invocations: each invocation renders current state. Reusing a
scheduled date key does not guarantee that a later payload is unchanged or that
Resend still retains the key. A manual send has a new identity. Clockwork
continuation does not extend provider idempotency or authorize a duplicate.

If scheduled admission meets a deployment hold, Todo returns success with
`data` equal to `{"scheduled":true,"skipped":"deployment_maintenance"}`.
No digest is sent and no failure incident is created. Other admission errors
remain failures. Empty digest content is a valid all-clear message.

## Authority and external effects

Todo owns digest rendering and installed sender/recipient configuration.
Resend owns external submission records, and the recipient provider owns final
delivery. Clockwork owns scheduled admission, the durable failure halt and
explicit continuation. It submits incident alerts through the installed Email
CLI to Email's fixed personal recipient. That alert uses bounded operational
metadata, not the digest body. Todo's digest still uses direct Resend transport
and its own configured sender and recipient. A successful send establishes
Resend acceptance; confirm receipt
separately when final delivery matters.

Sending discloses aggregate counts, every open canonical todo's current title,
generic plain-language stage labels, applicable typed references, and
read-only inspection commands to Resend and the configured recipient's
provider. The digest excludes concern bodies, directions, notes, source paths,
assessment and design summaries, unresolved-choice text, and evidence.

Preview does not authorize send. The API key must remain outside Todo
configuration, Clockwork definition and LaunchAgent plist. The same-release
zsh credential script reads `~/.zshrc` and launches the pinned Todo payload with
a scrubbed environment; the key is not passed as a process argument.

Neither send nor preview invokes Nucleus. If a scheduled send fails, inspect
`~/Library/Logs/Todo/email.stderr.log`, Clockwork's incident and Resend's records.
A user LaunchAgent cannot guarantee a 09:00 submission while the Mac is powered
off or the user is logged out.

## Rust callers

Use `todo::api::Client` with provider-owned request and response types.
The client invokes an explicitly selected CLI and decodes its envelopes. It
preserves this operation's effects, failures, and authority requirements and
does not retry automatically. Convert results only to caller-local models.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.

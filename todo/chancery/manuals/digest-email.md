# Preview or send the Todo daily attention digest

The digest is a current, read-only projection over unresolved captured
concerns and open canonical todos. It groups items by the attention they need:

- **Needs your decision** includes a routing proposal, situation choice, or
  desired-state design awaiting an explicit user decision.
- **Needs follow-up** includes an unresolved concern without a pending routing
  decision, or an open todo that needs assessment, reassessment, more evidence,
  or desired-state design work.
- **Other open todos** includes the remaining open todos. An accepted desired
  state still leaves its todo open and does not claim implementation.

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

Send requires `[email]` configuration and a nonblank, whitespace-clean
`RESEND_API_KEY` in the process environment. It freezes one body and
`todo-email/<UUIDv7>` idempotency key for up to three attempts on transport,
rate-limit, or server failures. It sends immediately; Todo has no delivery
database or background retry queue.

The installed daily LaunchAgent invokes:

```sh
/Users/joey/.local/bin/todo email send --scheduled
```

Scheduled mode uses `todo-daily-email/<LOCAL YYYY-MM-DD>` for the most recent
local 09:00 occurrence. It does not submit a Resend `scheduled_at` value. A
manual send uses a different key and does not consume the scheduled
occurrence.

## Authority and external effects

Todo owns digest rendering and installed sender/recipient configuration.
Resend owns external submission records, and the recipient provider owns final
delivery. A successful send establishes Resend acceptance; confirm receipt
separately when final delivery matters.

Sending discloses aggregate counts, every open canonical todo's current title,
generic plain-language stage labels, applicable typed references, and
read-only inspection commands to Resend and the configured recipient's
provider. The digest excludes concern bodies, directions, notes, source paths,
assessment and design summaries, unresolved-choice text, and evidence.

Preview does not authorize send. The API key must remain outside Todo
configuration and the LaunchAgent plist; the installed runner reads it from
the user's environment setup and launches Todo with a scrubbed environment.

Sending or previewing the digest does not invoke Nucleus and is not a Todo
requester canary. On scheduled failure, inspect
`~/Library/Logs/Todo/email.stderr.log` and Resend's records. A user LaunchAgent
cannot guarantee a 09:00 submission while the Mac is powered off or the user
is logged out.

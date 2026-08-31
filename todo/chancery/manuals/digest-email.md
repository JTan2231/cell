# Preview or send the Todo digest

The digest is a current projection over open canonical todo umbrellas, newest
first. It contains each todo ID and current title, with subject
`Todo: N outstanding`.

## Preview first

```sh
/Users/joey/.local/bin/todo email preview
```

Preview renders the exact configured message without reading
`RESEND_API_KEY` or making a network request. Human output includes From, To,
Subject, and plain-text body; JSON also exposes HTML and count. Use preview
when validating content or disclosure before any external action.

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

Sending discloses every open todo ID and title to Resend and the configured
recipient's provider. Preview does not authorize send. The API key must remain
outside Todo configuration and the LaunchAgent plist; the installed runner
reads it from the user's environment setup and launches Todo with a scrubbed
environment.

Sending or previewing the digest does not invoke Nucleus and is not a Todo
requester canary. On scheduled failure, inspect
`~/Library/Logs/Todo/email.stderr.log` and Resend's records. A user LaunchAgent
cannot guarantee a 09:00 submission while the Mac is powered off or the user
is logged out.

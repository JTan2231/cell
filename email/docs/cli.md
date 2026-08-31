# CLI contract

Email has one command shape:

```text
email <SUBJECT> <BODY>
```

`SUBJECT` and `BODY` are required positional UTF-8 strings. When `BODY` is
exactly `-`, Email reads the complete plain-text body from standard input.
There are no recipient, sender, HTML, attachment, copy, scheduling, or preview
options.

Every send uses:

```text
From: Codex <codex@joeytan.dev>
To:   j.tan2231@gmail.com
```

The command requires a nonblank, whitespace-clean `RESEND_API_KEY` environment
variable. One invocation freezes one `email/<UUIDv7>` idempotency key and uses
it for at most three attempts. Transport errors, HTTP 429, and server errors are
retried after two short bounded delays. Other Resend rejections fail
immediately.

On acceptance, stdout is:

```text
Sent <resend-message-id>
```

and the process exits zero. Errors use the `email: ` prefix on stderr, omit the
API key and response body, and exit nonzero. Acceptance is not proof of final
Gmail delivery.

# CLI contract

Email has one command shape:

```text
email [--idempotency-key KEY] <SUBJECT> <BODY>
```

`SUBJECT` and `BODY` are required positional UTF-8 strings. When `BODY` is
exactly `-`, Email reads the complete plain-text body from standard input.
There are no recipient, sender, HTML, attachment, copy, scheduling, or preview
options.

`--idempotency-key KEY` lets an authorized calling product identify one exact
send request. `KEY` must contain 1 to 256 visible ASCII characters and no
whitespace. Callers must not put secrets or message content in it. Reusing a
key with the same payload within Resend's 24-hour retention window deduplicates
the submission; reusing it with a different payload is an error. Email does not
persist the key or decide when it may be reused.

Every send uses:

```text
From: Codex <codex@joeytan.dev>
To:   j.tan2231@gmail.com
```

The command requires a nonblank, whitespace-clean `RESEND_API_KEY` environment
variable. Unless the caller supplies a key, one invocation creates one
`email/<UUIDv7>` key. The selected key and request are frozen for at most three
attempts. Transport errors, HTTP 429, and server errors are retried after two
short bounded delays. Other Resend rejections fail immediately.

On acceptance, stdout is:

```text
Sent <resend-message-id>
```

and the process exits zero. Errors use the `email: ` prefix on stderr, omit the
API key and response body, and exit nonzero. Acceptance is not proof of final
Gmail delivery.

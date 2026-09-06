# CLI contract

Email has one command shape:

```text
email [--idempotency-key KEY] [--attach PATH]... <SUBJECT> <BODY>
```

`SUBJECT` and `BODY` are required positional UTF-8 strings. When `BODY` is
exactly `-`, Email reads the complete plain-text body from standard input.
Optional `--attach PATH` may be repeated for local file attachments. There are
no recipient, sender, HTML, remote attachment URL, copy, scheduling, or preview
options.

An attachment path must identify a readable regular local file with a UTF-8
basename containing no control characters or path separators. Relative paths
are resolved against the caller's working directory. Files are read once
before the first network request. Email sends each basename and base64-encoded
content in argument order; no local source path is sent. File-read errors do not
print paths or contents. Missing or invalid files fail before submission.

```sh
email --idempotency-key 'packets/daily/2026-09-06' \
  --attach /absolute/job-one-resume.pdf --attach /absolute/job-two-resume.pdf \
  'Daily jobs' - < /absolute/body.txt
```

The entire attachment payload remains in memory until the command ends.
[Resend's send API](https://resend.com/docs/api-reference/emails/send-email)
currently limits each email to 40 MB after attachment base64 encoding and
applies its own file-type restrictions; Email does not promise acceptance of
every file or size. Filenames and bytes are disclosed to Resend and Gmail.

`--idempotency-key KEY` lets an authorized calling product identify one exact
send request. `KEY` must contain 1 to 256 visible ASCII characters and no
whitespace. Callers must not put secrets or message content in it. Reusing a
key with the same payload within Resend's 24-hour retention window deduplicates
the submission; reusing it with a different payload is an error. Email does not
persist the key or decide when it may be reused.
The payload includes attachment order, names, and bytes. Retries within one
invocation never reopen files. A later invocation reads the files anew, so the
calling product must retain their exact content and names for the same key.

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
Accepted <resend-message-id>
```

and the process exits zero. Errors use the `email: ` prefix on stderr, omit the
API key and response body, and exit nonzero. Acceptance is not proof of final
Gmail delivery.

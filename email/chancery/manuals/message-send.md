# Send a personal email now

Email immediately sends one plain-text message with optional local attachments.
The sender and recipient are fixed. Use Email when the user explicitly asks to
send the supplied or approved message. An installed product can also use Email
if its contract grants standing authority for that exact kind of notification.

## Send

For a short literal body:

```sh
/Users/joey/.local/bin/email 'Subject' 'Body'
```

For a multiline body, pass `-` and provide standard input:

```sh
/Users/joey/.local/bin/email 'Subject' - < /absolute/path/to/body.txt
```

An authorized product can supply a stable idempotency key for an occurrence
that it owns. The key must identify an unchanged payload:

```sh
/Users/joey/.local/bin/email \
  --idempotency-key 'product/event/2026-09-01' \
  'Subject' - < /absolute/path/to/body.txt
```

Attach a local file with `--attach PATH`, repeating the option for more files:

```sh
/Users/joey/.local/bin/email \
  --idempotency-key 'packets/daily/2026-09-06' \
  --attach /absolute/job-one-resume.pdf --attach /absolute/job-two-resume.pdf \
  'Daily jobs' - < /absolute/body.txt
```

Each attachment must be an authorized, readable regular local file with a
UTF-8 basename containing no control characters or path separators. Relative
paths use the caller's working directory. The CLI captures files once before
any network request and transmits only basenames and base64-encoded bytes,
never source paths. File-read errors do not echo paths or contents. Remote attachment
URLs are unsupported. The full payload stays in memory until the command
ends, and provider message-size and file-type limits still apply.

Attachment order, filenames, and bytes belong to the exact idempotent payload.
Retries within one invocation never reopen files. A later invocation reads
them again: the caller must preserve the exact files and ordering for the same
key. Email retains no attachment copy or send history after exit.

The key must contain 1 to 256 visible ASCII characters without whitespace.
It must identify that exact payload and contain no secret or message content.
Resend retains idempotency keys for 24 hours; the same key and payload
deduplicate, while a changed payload under the same key is rejected. Without
the option, Email generates a fresh `email/<UUIDv7>` key.

The installed wrapper reads `RESEND_API_KEY` from `~/.zshrc`, scrubs unrelated
caller environment variables, supplies a minimal runtime environment, and
preserves standard input for the payload. Do not place the API key in the
command arguments, message text, product files, or Chancery contract.

## Authority and result

Email accepts the caller's subject, body, and attachments. It fixes both
addresses and submits the message immediately to Resend. The calling product
owns standing authority, scheduling, occurrence state, rendering, and stable
key selection. Email has no draft, preview, scheduler, daemon, local send
history, or background retry queue.

A successful command and returned Resend message identifier prove submission
acceptance only. Resend owns that external acceptance record. Gmail owns final
delivery, spam classification, and inbox receipt. When the requested outcome
includes delivery confirmation, inspect Gmail separately rather than inferring
receipt from process exit.

Sending discloses the exact subject, body, and attached filenames and bytes to
Resend and Gmail. A supplied idempotency key is disclosed to Resend as well.
Email retains none of them
locally. Do not invoke it for drafting, revising, or discussing a message, for
another recipient, or for HTML, remote attachment URLs, carbon copies, scheduling, or
delivery tracking. The option does not itself authorize a send.

On input, credential, network, or Resend failure, preserve the error for
diagnosis without exposing the API key. One invocation retries transport
errors, rate limits, and server errors at most twice with the same frozen
request and idempotency key. Email retains no queued work to resume. After an
ambiguous transport failure, inspect Resend before explicitly sending again
when a duplicate would be harmful.

The Email runtime does not call Chancery. Chancery provides installed,
version-matched discovery documentation only and does not authorize or execute
the send.

## Output selection

When Resend accepts the message, Email exits zero and prints `Accepted` followed
by the message ID. This confirms submission acceptance. Check Gmail separately
for delivery. On failure, Email exits nonzero and reports a bounded error
without the response body or credential.

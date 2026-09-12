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

Email uses the shared Chancery usage writer for command metadata. Catalog
discovery remains separate and does not authorize or execute a send.

## Output selection

When Resend accepts the message, Email exits zero and prints `Accepted` followed
by the message ID. This confirms submission acceptance. Check Gmail separately
for delivery. On failure, Email exits nonzero and reports a bounded error
without the response body or credential.

## Byte payloads on stdin

Email also accepts an in-memory attachment handoff without local attachment
files:

```sh
email --payload-stdin --idempotency-key product/occurrence 'Subject' -
```

Standard input is one JSON object:

```json
{"body":"Exact plain-text message","attachments":[{"filename":"resume.pdf","content":"JVBERi0="}]}
```

`content` is standard base64 of the exact attachment bytes. Attachment order,
filenames, subject, and body are part of the idempotent payload. The example
bytes illustrate encoding and do not contain a real resume. `--payload-stdin`
requires body `-` and conflicts with `--attach`. Unknown fields, malformed JSON,
invalid base64, and unsafe filenames fail before any network request.

The payload stays in memory and is not written to disk. The wrapper preserves
stdin and uses its existing credential loading. Do not include credentials in
the JSON. This extension preserves the fixed addresses, bounded transport
retries, acceptance output, and requirement for send authorization. Email
retains no local attachment copy or send history after exit.

The flag is additive in Email 0.5.1 under attachment contract 4. A caller that
needs byte input must select an executable that advertises `--payload-stdin`.

## Reply routing and thread headers

Contract 4 also supports `--reply-to ADDRESS`, `--in-reply-to MESSAGE_ID`, and
repeated `--reference MESSAGE_ID` flags. These optional values belong to the
exact frozen request and must remain identical when the same idempotency key
is reused. They do not change Email's fixed sender or recipient.

The caller must authorize the reply mailbox. Email accepts one ASCII mailbox
without a display name, at most 254 bytes, with a dot-atom local part of at
most 64 bytes and DNS labels of at most 63 bytes. A Message-ID must be one
bracketed ASCII ID such as `<answer@example.com>`, at most 998 bytes, with no
whitespace, inner angle brackets, backslash or double quote. References
preserve caller order, with at most 64 IDs and 8192 bytes including spaces.
Email accepts no arbitrary email headers.

```sh
email --payload-stdin --idempotency-key mentor/critique/answer-id \
  --reply-to mentor-assignment@account.resend.app \
  --in-reply-to '<answer@example.com>' \
  --reference '<problem@example.com>' --reference '<answer@example.com>' \
  'Re: Design a rate limiter' -
```

`In-Reply-To` and `References` use RFC Message-IDs, not Resend provider IDs.
Resend and Gmail receive these fields. Gmail decides how the thread appears;
submission acceptance does not establish threading or final delivery. Email
retains no thread state. Receiving is separately documented by
`email.message.receive` and requires its own read authority.

Rust callers keep `Message`, `send` and `send_with_attachments`. The additive
`ReplyOptions` and `send_with_options` API carries the reply fields. Direct
free functions read the process credential. `api::Client::new` selects one
absolute installed wrapper and supplies the body and attachment bytes on
stdin; it does not read the credential. That client performs no process retry,
bounds each command to 120 seconds, and reads at most 4096 receipt bytes.
It discards child stderr and returns a bounded command error. A timeout can
leave send acceptance unknown. The HTTP transport does not follow redirects.

The new `receive list/get` CLI forms are reserved. Use `--` before literal send
positionals to avoid a collision, for example `email -- receive list`.

Email selects its explicitly configured private credential first, then the
existing `RESEND_API_KEY` environment fallback. Local setup and domain discovery
use the separate [account operation](account-operate.md).

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.

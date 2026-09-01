# Send a personal email now

Email sends one immediate plain-text message from its product-fixed sender to
its fixed personal recipient. Use it after the user explicitly asks to send
the supplied or approved message, or from an installed product whose contract
already grants standing authority for the exact kind of notification.

## Send

For a short literal body:

```sh
/Users/joey/.local/bin/email 'Subject' 'Body'
```

For a multiline body, pass `-` and provide standard input:

```sh
/Users/joey/.local/bin/email 'Subject' - < /absolute/path/to/body.txt
```

An authorized product that owns a particular occurrence and freezes its exact
payload can supply its stable idempotency key:

```sh
/Users/joey/.local/bin/email \
  --idempotency-key 'product/event/2026-09-01' \
  'Subject' - < /absolute/path/to/body.txt
```

The key must contain 1 to 256 visible ASCII characters without whitespace.
It must identify that exact payload and contain no secret or message content.
Resend retains idempotency keys for 24 hours; the same key and payload
deduplicate, while a changed payload under the same key is rejected. Without
the option, Email generates a fresh `email/<UUIDv7>` key.

The installed wrapper reads `RESEND_API_KEY` from `~/.zshrc`, scrubs unrelated
caller environment variables, supplies a minimal runtime environment, and
preserves standard input for the payload. Do not place the API key in the
command arguments, message text, product files, or Chancery contract.

## Authority and proof

Email accepts the caller-provided subject and body, fixes both addresses,
and submits the message immediately to Resend. A product caller, not Email,
owns standing authority, scheduling, occurrence state, rendering, and stable
key selection. Email has no draft, preview, scheduler, daemon, local send
history, or background retry queue.

A successful command and returned Resend message identifier prove submission
acceptance only. Resend owns that external acceptance record. Gmail owns final
delivery, spam classification, and inbox receipt. When the requested outcome
includes delivery confirmation, inspect Gmail separately rather than inferring
receipt from process exit.

Sending discloses the exact subject and body to Resend and Gmail. A supplied
idempotency key is disclosed to Resend as well. Email retains none of them
locally. Do not invoke it for drafting, revising, or discussing a message, for
another recipient, or for HTML, attachments, carbon copies, scheduling, or
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

# Send a personal email now

Email sends one immediate plain-text message from its product-fixed sender to
its fixed personal recipient. Use it only after the user explicitly asks to
send and the exact subject and body have been supplied or approved.

## Send

For a short literal body:

```sh
/Users/joey/.local/bin/email 'Subject' 'Body'
```

For a multiline body, pass `-` and provide standard input:

```sh
/Users/joey/.local/bin/email 'Subject' - < /absolute/path/to/body.txt
```

The installed wrapper reads `RESEND_API_KEY` from `~/.zshrc`, scrubs unrelated
caller environment variables, supplies a minimal runtime environment, and
preserves standard input for the payload. Do not place the API key in the
command arguments, message text, product files, or Chancery contract.

## Authority and proof

Email accepts the caller-provided subject and body, fixes both addresses,
and submits the message immediately to Resend. It has no draft, preview,
scheduler, daemon, local send history, or background retry queue.

A successful command and returned Resend message identifier prove submission
acceptance only. Resend owns that external acceptance record. Gmail owns final
delivery, spam classification, and inbox receipt. When the requested outcome
includes delivery confirmation, inspect Gmail separately rather than inferring
receipt from process exit.

Sending discloses the exact subject and body to Resend and Gmail. Email retains
neither locally. Do not invoke it for drafting, revising, or discussing a
message, for another recipient, or for HTML, attachments, carbon copies,
scheduling, or delivery tracking.

On input, credential, network, or Resend failure, preserve the error for
diagnosis without exposing the API key. One invocation retries transport
errors, rate limits, and server errors at most twice with the same frozen
request and idempotency key. Email retains no queued work to resume. After an
ambiguous transport failure, inspect Resend before explicitly sending again
when a duplicate would be harmful.

The Email runtime does not call Chancery. Chancery provides installed,
version-matched discovery documentation only and does not authorize or execute
the send.

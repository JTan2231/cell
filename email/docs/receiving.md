# Received email

Email reads messages from the Resend account selected by the installed
wrapper's `RESEND_API_KEY`. A read requires user authority or an installed
caller's standing authority for that account mail. The send recipient limit
does not restrict the receiving API to messages from that recipient.

```sh
email receive list --limit 100
email receive list --limit 100 --after PROVIDER_ID
email receive get PROVIDER_ID
```

`list` reads exactly one page. `limit` defaults to 100 and accepts 1 through
100. The request always sends an explicit limit to Resend. Records are ordered
from newer to older. `after` is the preceding page's last provider ID, excluded
from the selected page. It is not a processed marker or an acknowledgement.
The caller owns traversal and must inspect `has_more`. Pages are provider
observations, not an atomic account snapshot or a delivery-completeness claim.

Provider IDs and cursors accept 1 through 256 ASCII letters, digits, `-`, and
`_`. `get` selects exactly one provider ID and checks that the returned ID
matches it. A missing provider record is an error, not an empty answer.

## Output

Success prints one JSON object and exits zero. `list` returns
`ReceivedPage { data: [ReceivedMessage, ...], has_more: boolean }`.
`get` returns one `ReceivedMessage`:

```json
{
  "id": "provider-id",
  "message_id": "<answer@example.com>",
  "from": "Person <person@example.com>",
  "to": ["assignment@account.resend.app"],
  "subject": "Re: Design a rate limiter",
  "created_at": "2026-09-07T10:00:00Z",
  "text": "My answer",
  "html": null,
  "headers": {},
  "attachments": [],
  "cc": [],
  "bcc": [],
  "reply_to": [],
  "received_for": []
}
```

`id` identifies the Resend receiving record and is suitable for caller-owned
deduplication. `message_id` is the separate RFC Message-ID used for threading.
`created_at` is the provider's timestamp for that email, not Email's poll time.
Missing text and HTML are JSON null. A list is metadata-only: both bodies are
null and `headers` is empty. Retrieve the record to obtain available content.

Attachment entries contain `id`, nullable `filename`, `content_type`,
`content_id`, `content_disposition`, and nullable `size` in bytes. They are
metadata only. Email does not download attachment bytes or raw-mail links.
It requests `html_format=cid`, so inline images remain references rather than
embedded base64 images. It does not render HTML, fetch remote resources, remove
quoted text, authenticate the sender, route replies, or determine whether an
answer is eligible for grading. Headers, sender claims and bodies are untrusted.

## Limits, failure and retention

Each provider read has a 30-second request timeout. Transport errors, response
body interruptions, HTTP 429 and server errors receive at most two retries
with the same selection. Other rejections fail immediately. Redirects are not
followed. Each receiving response is capped at 8 MiB, including bodies and
metadata. Oversized and malformed responses fail without partial output.

The error channel omits credentials, remote response bodies and mail content.
Read errors exit nonzero. Retrying a read does not delete, acknowledge, or mark
an email processed. Email retains no page, cursor, body, attachment, queue, or
read history after the invocation. Resend owns its record availability and
retention; this interface makes no retention-horizon or freshness promise.
Caller stdout, logs, files, and any model service are separate retention
boundaries. Do not log body-bearing JSON unless that retention is authorized.

The Rust `Client` invokes one absolute installed wrapper path with a minimal
environment. It does not load the key. It bounds receiving stdout to 16 MiB
after JSON normalization, send receipts to 4096 bytes, and each command to 120
seconds. It discards child stderr and reports a bounded command failure; run
the same authorized operation through the CLI when provider diagnostics are
needed. A client timeout can leave send acceptance unknown. The client does
not automatically retry commands or infer that a failed process had no effect.

The corresponding free Rust functions run transport in the caller process and
read `RESEND_API_KEY` from that environment. They do not source shell files.
The CLI uses those functions. `Message`, `Receipt`, the old send functions,
attachment paths and byte payloads remain supported.

See Resend's [list API](https://resend.com/docs/api-reference/emails/list-received-emails),
[retrieve API](https://resend.com/docs/api-reference/emails/retrieve-received-email),
[pagination](https://resend.com/docs/api-reference/pagination), and
[reply headers](https://resend.com/docs/dashboard/receiving/reply-to-emails).

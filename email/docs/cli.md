# CLI contract

Email has these command shapes:

```text
email [--idempotency-key KEY] [--attach PATH]... [--reply-to ADDRESS]
      [--in-reply-to MESSAGE_ID] [--reference MESSAGE_ID]... <SUBJECT> <BODY>
email receive list --limit 100 [--after ID]
email receive get ID
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

The command requires `RESEND_API_KEY` in the environment. Its value must be
nonblank and whitespace-clean. Unless the caller supplies an
idempotency key, each invocation creates one
`email/<UUIDv7>` key. The selected key and request are frozen for at most three
attempts. Transport errors, HTTP 429, and server errors are retried after two
short bounded delays. Other Resend rejections fail immediately.

On acceptance, stdout is:

```text
Accepted <resend-message-id>
```

The process then exits zero. On failure, Email exits nonzero and writes an
error with the `email: ` prefix to stderr. Errors omit the API key and response
body. Resend acceptance does not confirm Gmail delivery.

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

## Reply routing and threads

`--reply-to ADDRESS` accepts one ASCII mailbox without a display name. It
selects where the recipient's email client directs a reply. It does not change
Email's fixed sender or recipient. The caller must authorize that address.

`--in-reply-to MESSAGE_ID` and repeated `--reference MESSAGE_ID` set only the
`In-Reply-To` and `References` headers. Each ID must be a bracketed ASCII
Message-ID, such as `<answer@example.com>`, with no whitespace, angle brackets
inside the ID, backslash or double quote. Its maximum size is 998 bytes.
References preserve caller order and accept at most 64 IDs and 8192 bytes,
including the space budget. Arbitrary headers and configurable send addresses
remain unsupported. A reply mailbox has at most 254 bytes, a dot-atom local
part of at most 64 bytes, and DNS domain labels of at most 63 bytes.

```sh
email --payload-stdin --idempotency-key mentor/critique/answer-id \
  --reply-to mentor-assignment@account.resend.app \
  --in-reply-to '<answer@example.com>' \
  --reference '<problem@example.com>' --reference '<answer@example.com>' \
  'Re: Design a rate limiter' -
```

The body remains plain text. All reply fields are part of the exact frozen
request and must remain unchanged with its idempotency key. Resend receives
them, and Gmail uses the headers when deciding how to display a thread.
Acceptance does not prove threading or final delivery.

`receive list`, `receive get`, and `receive --help` select the receive parser.
Use `--` before literal send positionals to avoid those reserved forms, such as
`email -- receive list`. Receive commands use the same installed credential
wrapper. See [receiving](receiving.md) for the JSON and access contract.

# Email

Email is a CLI that sends one caller-provided
subject and plain-text body immediately through Resend from
`Codex <codex@joeytan.dev>` to `j.tan2231@gmail.com`.

```sh
email 'Subject' 'Body'
email 'Subject' - < body.txt
email --idempotency-key 'decisions/daily/2026-09-01' 'Subject' - < body.txt
email --idempotency-key 'packets/daily/2026-09-06' --attach resume.pdf 'Jobs' - < body.txt
```

The second form reads the body from standard input. An authorized calling
product can supply one stable idempotency key for one exact message.
Interactive calls receive a new `email/<UUIDv7>` key. Email sends
immediately. There is no recipient option, draft store, HTML mode, remote attachment URL
support, scheduler, daemon, or delivery database. A successful command means
Resend accepted the submission; it does not prove final Gmail delivery.

Repeat `--attach PATH` for multiple local files. Each file is captured once
before sending; filenames and exact bytes are part of the idempotent request.
Only basenames and contents leave the machine, never local source paths.
Callers must retain the exact files when they own later retries.

## Build, test, and install

The macOS user installation reads `RESEND_API_KEY` from `~/.zshrc` without
putting the secret in command arguments or product state:

```sh
export RESEND_API_KEY='re_replace_with_the_real_key'

cd /Users/joey/rust/cell/email
./ci.sh
<TESTED_EMAIL_INSTALL> install \
  --binary <TESTED_EMAIL_BINARY> \
  --bundle /Users/joey/rust/cell/email/chancery
```

The installed command is `~/.local/bin/email`. Installation details and
recovery boundaries are in [docs/system-installation.md](docs/system-installation.md).
The [Chancery provider for this release](chancery/provider.json) lists all
supported send capabilities. Select `email.message.send`, then use
`chancery resolve email.message.send` to read its contract, external dependencies,
sources, and declared gaps. Resolution does not load credentials, check Resend,
or send a message.

Sending discloses the subject, body, and attachment names and bytes to Resend
and Gmail. The runtime does not call Chancery. Chancery reads the documentation
with the installed Email release.

## Rust interface

`email::api::{Message, Receipt, send}` exposes the same fixed-recipient,
plain-text submission as the CLI, which uses that implementation. Callers own
send authorization and any supplied occurrence key. `Receipt` means Resend
accepted the submission. The API does not add configurable addresses, HTML or
retained delivery state.

`Attachment { filename, content }` and `send_with_attachments(&message,
&attachments)` extend that interface without changing `Message` or `send`.
The caller supplies captured bytes and a basename; Email does not read files
through the Rust API or fetch remote attachments.

Callers holding attachment bytes in a database can use `--payload-stdin` with
body `-` to supply JSON containing the body and ordered base64 attachments.
See the [CLI contract](docs/cli.md). Email retains no local payload files.

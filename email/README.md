# Email

Email is a deliberately single-purpose CLI. It sends one caller-provided
subject and plain-text body immediately through Resend from
`Codex <codex@joeytan.dev>` to `j.tan2231@gmail.com`.

```sh
email 'Subject' 'Body'
email 'Subject' - < body.txt
email --idempotency-key 'decisions/daily/2026-09-01' 'Subject' - < body.txt
```

The second form reads the body from standard input. An authorized calling
product can supply one stable idempotency key for one exact message; ordinary
interactive calls receive a fresh `email/<UUIDv7>` key. Email still sends
immediately. There is no recipient option, draft store, HTML mode, attachment
support, scheduler, daemon, or delivery database. A successful command means
Resend accepted the submission; it does not prove final Gmail delivery.

## Build, test, and install

The macOS user installation reads `RESEND_API_KEY` from `~/.zshrc` without
putting the secret in command arguments or product state:

```sh
export RESEND_API_KEY='re_replace_with_the_real_key'

cd /Users/joey/rust/cell/email
./ci.sh
./packaging/macos/deploy-user.sh \
  --binary /Users/joey/rust/cell/target/release/email
```

The installed command is `~/.local/bin/email`. Installation details and
recovery boundaries are in [docs/system-installation.md](docs/system-installation.md).
The globally discoverable usage contract is in
[chancery/manuals/message-send.md](chancery/manuals/message-send.md).

Sending discloses the supplied subject and body to Resend and Gmail. The
runtime does not call Chancery; Chancery only reads the documentation staged
with the installed Email release.

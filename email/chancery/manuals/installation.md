# Email program installation

The installer copies the supplied programs and provider bundle into a retained
release and selects their owned public paths together. It creates required
installation directories and uses product and catalog locks with atomic selector
updates. `--expected-current absent|releases/ID` guards the selected release.
Foreign public selectors are refused. File-operation or basic execution failures
restore the prior selectors when possible.

Opaque UUID release IDs name the staged files. Installation and recovery do not compare
artifact hashes, component versions, or retained file inventories. They do not
run database integrity checks, dependency probes, or readiness checks. Basic
`--help` and `--version` execution checks remain. Inspection reads recorded
installation metadata and selectors; it is not an integrity result.

The default installation root is
`~/Library/Application Support/Email/install`. Releases are retained beneath
`releases/ID`; `current` selects the program and provider together and `previous`
retains the superseded selection. Public commands are `~/.local/bin/email` and
`~/.local/bin/email-install`. The provider selector is
`~/Library/Application Support/Chancery/providers/email`.

```sh
email-install install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE
email-install inspect
email-install recover --release ABSOLUTE_RELEASE_DIRECTORY
```

Use `--home ABSOLUTE_HOME` for an intentional alternate user home. Recovery reads
retained metadata and selects a release in that home's installation directory.
It does not rebuild the release or restore product data. There is no installer
`verify` or `verify-release` command. Ordinary runtime checks keep their existing
behavior.

Private account settings remain outside program releases. Email installs no daemon,
schedule, queue, remote account, or key.

## Wrapper and credential boundaries

Email first selects its configured private credential. The wrapper provides the
existing `RESEND_API_KEY` fallback by sourcing `~/.zshrc` and extracting only
that variable. It starts the payload with a scrubbed environment containing
the key, `HOME`, a fixed system `PATH`, and ordinary shell bookkeeping variables.
Unrelated caller credentials are not forwarded. The wrapper preserves stdin;
credentials are not command arguments or part of the program release.

Help and version probes bypass `.zshrc` and execute the release payload
directly. Installation and recovery use only those probes. They do not load
transport credentials, read account mail, send messages, or prove Resend/Gmail
readiness. The credential fallback remains supported alongside explicit setup.

## Usage and recovery

Run `email --register-usage` separately after installation. It reads or sends no
mail. Program recovery preserves private account settings. Read `email.account`
for account setup and `email.message.send` for sending behavior.

No installation-time account readiness, final-delivery guarantee, retention
horizon, or general cross-release support interval is promised.

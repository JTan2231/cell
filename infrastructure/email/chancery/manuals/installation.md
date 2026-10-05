# Email program installation

Submit deployments from the Cell root with
`./ci.sh submit COMMIT --deploy email` and verify the retained manager job
outcome. CI and Telete are the deployment route. The installer APIs below are
underlying product setup and explicit retained-release recovery interfaces.

The installer retains the supplied programs and provider bundle in a release
archive. It publishes regular executable files beneath
`~/Library/Application Support/Email/install/runtime/`. Updates and recovery
replace those files at the same actual paths. Public command selectors use this runtime tree. Provider directory selectors
use the selected archive, so each catalog read selects one retained bundle. The installer uses product and catalog locks;
each file replacement is atomic. The complete tree is not one atomic update.
`--expected-current absent|releases/ID` guards the recorded release selection.
Foreign public selectors are refused. An instruction failure retains completed
file and selector changes for explicit recovery.

Opaque UUID release IDs name retained archives. Installation and recovery do not compare
artifact hashes, component versions, or retained file inventories. They do not
run executable probes, native-signature audits, database integrity checks,
dependency probes, or readiness checks. Inspection reads recorded installation
metadata and selectors; it is not an integrity result.

The default installation root is
`~/Library/Application Support/Email/install`. Releases are retained beneath
`releases/ID`; `current` records the selected archive and `previous`
retains the superseded archive. Programs execute from fixed runtime paths.
Public commands are `~/.local/bin/email` and
`~/.local/bin/email-install`. The provider selector is
`~/Library/Application Support/Chancery/providers/email`.

```sh
email-install install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE
email-install inspect
email-install recover --release ABSOLUTE_RELEASE_DIRECTORY
```

Use `--home ABSOLUTE_HOME` for an intentional alternate user home. Recovery reads
retained metadata and selects a release in that home's installation directory.
It republishes the retained files at the same runtime paths without rebuilding
the archive or restoring product data. There is no installer
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

Explicit help and version commands bypass `.zshrc` and execute the payload
directly. Installation and recovery run no program probes. They do not read
account mail, send messages, or prove Resend/Gmail readiness. The credential fallback remains supported alongside explicit setup.

## Usage and recovery

Run `email --register-usage` separately after installation. It reads or sends no
mail. Program recovery preserves private account settings. Read `email.account`
for account setup and `email.message.send` for sending behavior.

No installation-time account readiness, final-delivery guarantee, retention
horizon, or general cross-release support interval is promised.

## Deployment recipe

`email-install deploy` reads one schema-two Cell recipe request from stdin.
The product command selects supplied programs and applies explicitly supplied private account settings.
The manifest executor runs this instruction and records its exit status. It does
not inspect application output or create a maintenance hold, drain work, or
recover prior effects. A failed instruction leaves completed changes in place.
Use the product's explicit recovery operation when recovery is required.

# User-owned macOS installation

Email sends during each invocation. Its only retained state is immutable
install releases and their selectors. It has no daemon or
delivery database.

An upstream product may own scheduling, occurrence state, message rendering,
and standing send authority. It can invoke Email with
`--idempotency-key KEY`, but Email remains an immediate transport and does not
acquire any of those responsibilities.

## Deploy

Verify `joeytan.dev` in Resend, create a sending-capable API key, and keep the
key in the installed user's `~/.zshrc`:

```sh
export RESEND_API_KEY='re_replace_with_the_real_key'
```

Run the product gate. Then install the exact binary and Rust installer that it tested:

```sh
cd /Users/joey/rust/cell/email
./ci.sh
<TESTED_EMAIL_INSTALL> install \
  --binary <TESTED_EMAIL_BINARY> \
  --bundle /Users/joey/rust/cell/email/chancery
```

The installed layout is:

```text
~/.local/bin/email -> Email's current release wrapper
~/.local/bin/email-install -> Email's current Rust installer
~/Library/Application Support/Email/install/
  releases/<content-hash>/
    bin/email
    libexec/email
    bin/email-install
    package/install
    package/email
    share/chancery/email/
    manifest.json  (cell-install-v2)
  current -> releases/<content-hash>
  previous -> releases/<content-hash>
~/Library/Application Support/Chancery/providers/
  email -> Email's current release share/chancery/email
```

The wrapper sources `~/.zshrc`, extracts only `RESEND_API_KEY`, and starts the
payload with a scrubbed environment containing that key, `HOME`, a fixed
system `PATH`, and ordinary shell bookkeeping variables. Other caller
credentials are not forwarded. The wrapper preserves standard input, and the
secret is not stored in Email files or passed in command arguments.
Help and version probes bypass `.zshrc` and execute the release payload
directly, so an upstream readiness check does not read transport credentials.

The release identity covers the payload, wrapper, Rust installer, public layout,
and Chancery
provider bundle. The installer validates an existing release before reuse,
retains the superseded release through `previous`, rejects a provider selector
owned by another installation, and restores all selectors when a post-switch
check fails. For rollback, resolve `install/previous` to its canonical owned
release directory, then run a trusted tested `email-install recover --release
ABSOLUTE_RELEASE_DIRECTORY`. The installer verifies the retained legacy or
`cell-install-v2` release before selecting it; do not execute an unverified
retained installer. Installation and recovery run only help/version probes and
never source `.zshrc` or send a message.

The installer creates Email's provider selector whether or not Chancery is
installed. Email remains usable without the Chancery binary or registry. After
deployment, Chancery discovery can be checked separately:

```sh
/Users/joey/.local/bin/chancery doctor
/Users/joey/.local/bin/chancery show email.message.send
/Users/joey/.local/bin/chancery resolve email.message.send
```

`resolve` reads the provider scope, send contract, external dependencies,
sources, and declared gaps for the installed release.
It does not source the Email credential, probe Resend or Gmail, or authorize or
perform a send.

## Validate a real send

Use a harmless, uniquely identifiable message and then confirm it in Gmail:

```sh
email 'Email CLI validation' 'The installed Email CLI can send through Resend.'
```

For a product-owned occurrence, use the product's stable key with its frozen
payload:

```sh
email --idempotency-key 'product/event/2026-09-01' 'Subject' - < body.txt
```

Command success confirms Resend acceptance. Check Gmail separately for receipt.
Sending discloses the subject, body, and attachment names and bytes to Resend
and Gmail. It also discloses a caller key to Resend. Email retains none of them locally.

## Supplied account settings

Use `email setup --credential-file ABS_PRIVATE_FILE --receiving-domain DOMAIN`,
or supply `credential_file` and `receiving_domain` in Email's Cell deployment
settings. Omitted fields remain unchanged. The credential is retained privately
by Email and takes precedence over the wrapper's environment fallback. Neither
path creates a remote key, changes DNS, reads mail or sends a message. See the
[complete account operation](../chancery/manuals/account-operate.md) for private
file requirements, repeatable failure recovery and receiving-domain discovery.

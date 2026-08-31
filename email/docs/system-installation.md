# User-owned macOS installation

Email is synchronous. It owns no daemon, configuration, delivery database, or
state beyond immutable install releases and their selectors.

## Deploy

Verify `joeytan.dev` in Resend, create a sending-capable API key, and keep the
key in the installed user's `~/.zshrc`:

```sh
export RESEND_API_KEY='re_replace_with_the_real_key'
```

Then run the product gate and deploy the release binary:

```sh
cd /Users/joey/rust/cell/email
./ci.sh
./packaging/macos/deploy-user.sh \
  --binary /Users/joey/rust/cell/target/release/email
```

The installed layout is:

```text
~/.local/bin/email -> Email's current release wrapper
~/Library/Application Support/Email/install/
  releases/<content-hash>/
    bin/email
    libexec/email
    package/deploy-user.sh
    package/email
    share/chancery/email/
    manifest.txt
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

The release identity covers the payload, wrapper, deployer, and Chancery
provider bundle. The installer validates an existing release before reuse,
retains the superseded release through `previous`, rejects a provider selector
owned by another installation, and restores all selectors when a post-switch
check fails. Re-deploying the binary retained by `previous` is the rollback
procedure.

The installer creates Email's provider selector whether or not Chancery is
installed. Email remains usable without the Chancery binary or registry. After
deployment, Chancery discovery can be checked separately:

```sh
/Users/joey/.local/bin/chancery doctor
/Users/joey/.local/bin/chancery show email.message.send
```

## Validate a real send

Use a harmless, uniquely identifiable message and then confirm it in Gmail:

```sh
email 'Email CLI validation' 'The installed Email CLI can send through Resend.'
```

Command success proves Resend acceptance. Gmail receipt must be observed
separately. Sending discloses the exact subject and body to Resend and Gmail;
Email retains neither one locally.

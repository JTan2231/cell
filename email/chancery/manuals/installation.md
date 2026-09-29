# Email installation and recovery guarantees

Email is a current-user macOS command. It submits or reads mail during an
invocation and has no daemon, delivery database, scheduler, or queue. Email
owns immutable installed releases, their selectors, and separately retained
private account settings. Requesters own schedules, occurrences, message
rendering, and standing authority.

Read `chancery show email.install.operate` for installation and rollback steps.
Read `chancery show email.account` for credential and domain-setting semantics.

## Installed layout

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

The release identity covers the payload, wrapper, Rust installer, public layout,
and complete Chancery provider bundle. Documentation follows the program
selection. Account settings under
`~/Library/Application Support/Email/settings` are separate from immutable
program releases. Program selection does not imply account or network readiness.

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

## Publication and selector consistency

Ordinary delivery uses Cell's CI manager with a committed source candidate.
An explicitly authorized manual installation or recovery uses trusted tested
installer and binary artifacts plus their matching provider bundle:

```sh
<TESTED_EMAIL_INSTALL> install --binary <TESTED_EMAIL_BINARY> --bundle <TESTED_EMAIL_BUNDLE>
```

The installer validates an existing immutable release before reuse, retains the
superseded release through `previous`, and rejects a provider selector owned by
another installation. A failed post-switch check restores all selectors.
Identical artifacts reuse the same content-addressed release.

The installer creates Email's provider selector even when Chancery is absent.
Email remains usable without the Chancery binary or registry. Chancery reads
the installed release's documentation without loading credentials, probing
providers, or performing the documented command.

`email --register-usage` registers the command inventory after installation or
update. Registration reads or sends no mail and does not establish readiness.

## Recovery and limits

The supported rollback selects the canonical owned release named by
`install/previous` through a trusted tested installer:

```sh
<TESTED_EMAIL_INSTALL> recover --release ABSOLUTE_RELEASE_DIRECTORY
```

The installer verifies the retained legacy or `cell-install-v2` release before
selection. Do not execute an unverified retained installer. Retain selector and
verification errors and follow the procedure instead of rewriting release files
or taking over foreign selectors.

Email retains no message history to recover after a send or receiving read.
Private account settings have their own atomic selection and recovery boundary.
Resend acceptance and Gmail delivery must be observed separately from installer
success. No installation-time account readiness, final-delivery guarantee,
release-retention horizon, or general cross-release support interval is promised.

## Related contracts

- Read `chancery show email.install.operate` for installation and recovery steps.
- Read `chancery show email.account` for separately retained settings.
- Read `chancery show email.message.send` for send results and ambiguous failures.
- Read `chancery show email.message.receive` for receiving observations.
- Read `chancery show ci-manager.queue.operate` for ordinary Cell delivery.

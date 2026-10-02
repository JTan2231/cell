# Usher program installation

The installer copies the supplied programs and provider bundle into a retained
release and selects their owned public paths together. It creates required
installation directories and uses product and catalog locks with atomic selector
updates. `--expected-current absent|releases/ID` guards the selected release.
Foreign public selectors are refused. An instruction failure retains completed
file and selector changes for explicit recovery.

Opaque UUID release IDs name the staged files. Installation and recovery do not compare
artifact hashes, component versions, or retained file inventories. They do not
run executable probes, native-signature audits, database integrity checks,
dependency probes, or readiness checks. Inspection reads recorded installation
metadata and selectors; it is not an integrity result.

The default installation root is
`~/Library/Application Support/Usher/install`. Releases are retained beneath
`releases/ID`; `current` selects the program and provider together and `previous`
retains the superseded selection. Public commands are `~/.local/bin/usher` and
`~/.local/bin/usher-install`. The provider selector is
`~/Library/Application Support/Chancery/providers/usher`.

```sh
usher-install install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE
usher-install inspect
usher-install recover --release ABSOLUTE_RELEASE_DIRECTORY
```

Use `--home ABSOLUTE_HOME` for an intentional alternate user home. Recovery reads
retained metadata and selects a release in that home's installation directory.
It does not rebuild the release or restore product data. There is no installer
`verify` or `verify-release` command. Ordinary runtime checks keep their existing
behavior.

Installation creates no semantic project, database, worker, schedule, or credentials.
It makes no network or model call and performs no membership assessment.

Usher writes `cell-install-simple-v2` manifests. Recovery also reads earlier JSON
metadata and the
legacy `manifest.txt` layout. A legacy release has no installer binary, so its
selection detaches the owned `usher-install` selector. Keep a Rust installer to
select a later release. Direct installation retains releases; cleanup is a separate explicit retention operation.

Run `usher --register-usage` separately after installation. Registration records
command identities and performs no recognition work.

Installation retains executable files, documentation, and release metadata. It
retains no credentials or recognition document bodies. Keep local paths and
release metadata within the caller's intended disclosure boundary. No installation
duration, retention horizon, or release cadence is promised.

## Deployment recipe

`usher-install deploy` reads one schema-two Cell recipe request from stdin.
The product command selects supplied programs and the provider.
The manifest executor runs this instruction and records its exit status. It does
not inspect application output or create a maintenance hold, drain work, or
recover prior effects. A failed instruction leaves completed changes in place.
Use the product's explicit recovery operation when recovery is required.

# Usher program installation

The installer copies the supplied programs and provider bundle into a retained
release and selects their owned public paths together. It creates required
installation directories and uses product and catalog locks with atomic selector
updates. `--expected-current absent|releases/HASH` guards the selected release.
Foreign public selectors are refused. File-operation or basic execution failures
restore the prior selectors when possible.

Release hashes name the staged files. Installation and recovery do not compare
artifact hashes, component versions, or retained file inventories. They do not
run database integrity checks, dependency probes, or readiness checks. Basic
`--help` and `--version` execution checks remain. Inspection reads recorded
installation metadata and selectors; it is not an integrity result.

The default installation root is
`~/Library/Application Support/Usher/install`. Releases are retained beneath
`releases/HASH`; `current` selects the program and provider together and `previous`
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

Usher retains its `cell-install-v1` manifest layout. Recovery also supports the
legacy `manifest.txt` layout. A legacy release has no installer binary, so its
selection detaches the owned `usher-install` selector. Keep a Rust installer to
select a later release. Direct installation retains releases; cleanup belongs
to the coordinator.

Run `usher --register-usage` separately after installation. Registration records
command identities and performs no recognition work.

Installation retains executable files, documentation, and release metadata. It
retains no credentials or recognition document bodies. Keep local paths and
release metadata within the caller's intended disclosure boundary. No installation
duration, retention horizon, or release cadence is promised.

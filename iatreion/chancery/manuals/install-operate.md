# Iatreion program installation

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
`~/Library/Application Support/Iatreion/install`. Releases are retained beneath
`releases/HASH`; `current` selects the program and provider together and `previous`
retains the superseded selection. Public commands are `~/.local/bin/iatreion` and
`~/.local/bin/iatreion-install`. The provider selector is
`~/Library/Application Support/Chancery/providers/iatreion`.

```sh
iatreion-install install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE
iatreion-install inspect
iatreion-install recover --release ABSOLUTE_RELEASE_DIRECTORY
```

Use `--home ABSOLUTE_HOME` for an intentional alternate user home. Recovery reads
retained metadata and selects a release in that home's installation directory.
It does not rebuild the release or restore product data. There is no installer
`verify` or `verify-release` command. Ordinary runtime checks keep their existing
behavior.

Installation creates no database, service, schedule, cache, or report. It does
not invoke product status probes or register a semantic project.

Use `cell-ci submit COMMIT` for ordinary delivery and inspect the retained manager
outcome. For manual installation, use supplied absolute binary and bundle paths.
Run `iatreion --register-usage` separately after selection.

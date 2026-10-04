# Iatreion program installation

The installer retains the supplied programs and provider bundle in a release
archive. It publishes regular executable files beneath
`~/Library/Application Support/Iatreion/install/runtime/`. Updates and recovery
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
`~/Library/Application Support/Iatreion/install`. Releases are retained beneath
`releases/ID`; `current` records the selected archive and `previous`
retains the superseded archive. Programs execute from fixed runtime paths.
Public commands are `~/.local/bin/iatreion` and
`~/.local/bin/iatreion-install`. The provider selector is
`~/Library/Application Support/Chancery/providers/iatreion`.

```sh
iatreion-install install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE
iatreion-install inspect
iatreion-install recover --release ABSOLUTE_RELEASE_DIRECTORY
```

Use `--home ABSOLUTE_HOME` for an intentional alternate user home. Recovery reads
retained metadata and selects a release in that home's installation directory.
It republishes the retained files at the same runtime paths without rebuilding
the archive or restoring product data. There is no installer
`verify` or `verify-release` command. Ordinary runtime checks keep their existing
behavior.

Installation creates no database, service, schedule, cache, or report. It does
not invoke product status probes or register a semantic project.

Use `cell-ci submit COMMIT` for ordinary delivery and inspect the retained manager
outcome. For manual installation, use supplied absolute binary and bundle paths.
Run `iatreion --register-usage` separately after selection.

## Deployment recipe

`iatreion-install deploy` reads one schema-two Cell recipe request from stdin.
The product command selects supplied programs and its provider.
The manifest executor runs this instruction and records its exit status. It does
not inspect application output or create a maintenance hold, drain work, or
recover prior effects. A failed instruction leaves completed changes in place.
Use the product's explicit recovery operation when recovery is required.

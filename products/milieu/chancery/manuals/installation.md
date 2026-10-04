# Milieu program installation

The installer retains the supplied programs and provider bundle in a release
archive. It publishes regular executable files beneath
`~/Library/Application Support/Milieu/install/runtime/`. Updates and recovery
replace those files at the same actual paths. Public command selectors use this runtime tree. Provider directory selectors
use the selected archive, so each catalog read selects one retained bundle. The installer uses product and catalog locks;
each file replacement is atomic. The complete tree is not one atomic update.
`--expected-current absent|releases/ID` guards the recorded release selection.
Foreign public selectors are refused. An instruction failure retains completed
file and selector changes for explicit recovery.

Opaque UUID release IDs name retained archives. Installation and recovery do not
compare artifact hashes, component versions, or retained file inventories. They
do not run executable probes, native-signature audits, database integrity
checks, dependency probes, or readiness checks. Inspection reads recorded
installation metadata and selectors; it is not an integrity result.

The default installation root is
`~/Library/Application Support/Milieu/install`. Releases are retained beneath
`releases/ID`; `current` records the selected archive and `previous`
retains the superseded archive. Programs execute from fixed runtime paths.
Public commands are `~/.local/bin/milieu` and
`~/.local/bin/milieu-install`. The provider selector is
`~/Library/Application Support/Chancery/providers/milieu`.

```sh
milieu-install install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE
milieu-install inspect
milieu-install recover --release ABSOLUTE_RELEASE_DIRECTORY
```

Use `--home ABSOLUTE_HOME` for an intentional alternate user home. Recovery
reads retained metadata and selects a release in that home's installation
directory. It republishes the retained files at the same runtime paths without rebuilding
the archive or restoring product data. There is no
installer `verify` or `verify-release` command. Run ordinary diagnostics
separately when requested.

Direct installation creates no database, schedule, or provider request. Milieu
performs no collection.

## Frontend environment

The installed zsh frontend starts the payload with `HOME`, fixed system
`PATH`, optional `MILIEU_STATE_DIR`, and the command-usage values
`CODEX_THREAD_ID`, `CHANCERY_USAGE_DB`, `CHANCERY_USAGE_DISABLED`, and
`CHANCERY_USAGE_INTERNAL`. It preserves arguments and standard input.

The frontend does not source `~/.zshrc` or extract provider keys. Milieu reads no
TheirStack or Brave credentials. Shell configuration is not a Milieu runtime
readiness dependency.

## Cell deployment setup

The Milieu deployment recipe initializes state after selecting its release.
Setup uses Milieu's native state APIs and product lock. It initializes missing
state with database schema 2 and preserves existing schema-two records without
collection. Unsupported database schemas cause an explicit failure.

The optional `state_dir` setting is an absolute path. State selection uses
`state_dir`, then a nonempty inherited `MILIEU_STATE_DIR`, then
`~/.local/share/milieu` in the selected home. Collector configuration and the
`config_file` setting are removed.

Setup does not invoke the selected CLI or source shell configuration. Explicit
retained-release recovery selects programs and leaves state unchanged.
Deployment creates no collection schedule. Read `milieu.state` for private
state selection, supported schemas, readiness, and recovery.

## Command usage

After each installation or update, run `milieu --register-usage`. This separate
operation registers the declared command inventory without product work.
Binary selection and catalog publication do not register it.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

## Compatibility and limits

Provider release, feature contract version, installation format, database
schema, and export schema evolve separately. The exact bundle is published and
recovered with its matching product release. Catalog presence establishes
neither live readiness nor permission to invoke Milieu.

Installation contract 4 removes shell credential loading and deployment
configuration. The retained release layout and supported installation-metadata
readers remain. Program recovery leaves data unchanged and does not check
program/state compatibility. Select a program that accepts the retained schema.
Older releases that require stored collector configuration can fail status or
diagnosis on newly initialized Milieu 0.6.0 state. Recovery does not recreate that
configuration; read `milieu.state` before selection.

No installation-latency objective, future migration promise, automatic state
rollback, or deprecation window is defined. Milieu installs no recurring
activation and has no Nucleus, CRM, Email, or computer-use runtime dependency.

## Deployment recipe

`milieu-install deploy` reads one schema-two Cell recipe request from stdin.
The product command selects supplied programs and initializes missing state
through the product lock. The manifest executor runs this instruction and
records its exit status. It does not inspect application output or create a
maintenance hold, drain work, or recover prior effects. A failed instruction
leaves completed changes in place. Use explicit product recovery when required.

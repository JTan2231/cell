# Cast program installation

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
`~/Library/Application Support/Cast/install`. Releases are retained beneath
`releases/ID`; `current` selects the program and provider together and `previous`
retains the superseded selection. Public commands are `~/.local/bin/cast` and
`~/.local/bin/cast-install`. The provider selector is
`~/Library/Application Support/Chancery/providers/cast`.

```sh
cast-install install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE
cast-install inspect
cast-install recover --release ABSOLUTE_RELEASE_DIRECTORY
```

Use `--home ABSOLUTE_HOME` for an intentional alternate user home. Recovery reads
retained metadata and selects a release in that home's installation directory.
It does not rebuild the release or restore product data. There is no installer
`verify` or `verify-release` command. Ordinary runtime checks keep their existing
behavior.

Installation creates no database, schedule, or provider request. Cast performs
no collection in this release.

## Frontend credentials and trust

The retained diagnostic path reads `THEIRSTACK_API_KEY` and
`BRAVE_SEARCH_API_KEY` presence from its environment without using the keys
for collection. The installed zsh frontend suppresses trace and output while it
sources `~/.zshrc`. It extracts those keys and starts the payload with only
`HOME`, fixed system `PATH`, optional `CAST_STATE_DIR`, the two provider keys,
and the command-usage values `CODEX_THREAD_ID`, `CHANCERY_USAGE_DB`,
`CHANCERY_USAGE_DISABLED`, and `CHANCERY_USAGE_INTERNAL`. State selection and
usage values are captured before sourcing shell configuration. The frontend
preserves arguments and standard input. Help, version, and `--register-usage`
reads bypass shell configuration.

No key appears in an argument, saved config, provider contract, or command
output. `.zshrc` is user-owned executable shell configuration. Its commands
and side effects remain the user's responsibility. Chancery does not execute
or validate it, and no dedicated installed contract establishes its readiness.
Do not place credentials in query configuration, database rows, or logs.

## Cell deployment setup

The Cast deployment recipe configures state after selecting its release. Setup uses
Cast's native state APIs and product lock. It initializes missing state and
preserves existing records and consumed budgets without collecting. Optional
settings are `state_dir` and `config_file`, both absolute paths. A supplied
`config_file` replaces the complete configuration. Omitted settings retain
current values. State selection uses `state_dir`, then a nonempty inherited
`CAST_STATE_DIR`, then `~/.local/share/cast` in the selected home.

Setup does not invoke the selected CLI or source shell configuration. Explicit
retained-release recovery selects programs and leaves state configuration unchanged.
Deployment creates no collection schedule. Setup retains the existing
discovery schema; it neither activates the unused current model definitions nor
migrates existing data into them.

Read `cast.state` for private state selection, configuration, and state
recovery. Program installation and discovery state have separate lifecycles.

## Command usage

After each installation or update, run `cast --register-usage`. This separate
operation registers the declared command inventory without product work.
Binary selection and catalog publication do not register it.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

## Compatibility and limits

Provider release, feature contract version, installation format, database
schema, configuration schema, and export schema evolve separately. The exact
bundle is published and recovered with its matching product release. Catalog
presence establishes neither live readiness nor permission to invoke Cast.

No installation-latency objective, future migration promise, automatic state
rollback, or deprecation window is defined. Cast installs no recurring
activation and has no Nucleus, CRM, Email, or computer-use runtime dependency.

## Deployment recipe

`cast-install deploy` reads one schema-two Cell recipe request from stdin.
The product command selects supplied programs, initializes missing state, and applies supplied configuration through the product lock.
The manifest executor runs this instruction and records its exit status. It does
not inspect application output or create a maintenance hold, drain work, or
recover prior effects. A failed instruction leaves completed changes in place.
Use the product's explicit recovery operation when recovery is required.

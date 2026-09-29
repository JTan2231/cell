# Cast installation and program recovery

Cast publishes its Rust payload, static zsh frontend, Rust installer, and
matching Chancery provider bundle as one content-addressed release. Cast owns
only its program and provider selectors. Use `cast.install.operate` for the
installation and recovery procedure.

## Installer interfaces

```sh
<TESTED_CAST_INSTALL> install --binary <TESTED_CAST_BINARY> \
  --bundle /absolute/path/to/cast/chancery \
  --expected-current absent
cast-install recover --release ABSOLUTE_RELEASE_DIRECTORY
```

Use a trusted tested installer and a validated candidate. `install` requires
absolute `--binary` and `--bundle` paths. `--home PATH` selects the operator
home. `--expected-current absent|releases/HASH` requires an exact expected
selection. `recover` accepts a canonical owned retained release directory.
These inputs select programs; they do not authorize collection or state
replacement.

## Release identity and selection

The installer stages the exact payload, frontend, installer, and provider
bundle under `~/Library/Application Support/Cast/install/releases/HASH`.
Their bytes participate in release identity and integrity verification. The
`cell-install-v2` manifest is `manifest.json`; `package/install` retains the
Rust installer.

The installed `~/.local/bin/cast`, `~/.local/bin/cast-install`, and Cast
provider selector follow one atomic `current` release. The deployer takes
product and catalog writer locks, refuses foreign selectors, checks
candidate/provider versions, and restores prior selectors after a failed
switch. Chancery is installed documentation; Cast requires no Chancery runtime
to collect or read records.

Installation creates no discovery database, daemon, LaunchAgent, scheduler
binding, provider request, model job, or downstream workflow. It performs no
live-state migration. Program identity and help/version checks do not prove
provider authentication or current collection readiness.

## Frontend credentials and trust

The Rust payload reads `THEIRSTACK_API_KEY` and `BRAVE_SEARCH_API_KEY` from its
environment. The installed zsh frontend suppresses trace and output while it
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

The Cell coordinator configures Cast after selecting its release. Configuration
runs `cast init`, which initializes missing state and preserves existing
records and consumed budgets without collecting. Optional settings are
`state_dir` and `config_file`, both absolute paths. A supplied `config_file`
replaces the complete configuration through `config set --file`. Omitted
settings retain current values. Deployment creates no collection schedule.

Read `cast.state` for private state selection, configuration, and state
recovery. Program installation and discovery state have separate lifecycles.

## Failure and program recovery

Prior releases remain available. Resolve `install/previous` to its canonical
owned release directory, then select it with a trusted tested
`cast-install recover --release ABSOLUTE_RELEASE_DIRECTORY`. The installer
verifies the retained legacy or `cell-install-v2` release before selection.
Do not execute an unverified retained installer or edit an installed
content-addressed bundle.

Program recovery leaves discovery state and consumed allowance unchanged. It
does not restore a database/configuration pair. An older program can reject a
newer configuration field; verify compatibility before selection.

An abruptly killed deployer can leave its `.update-lock` directory. Confirm
that no Cast deployer is running before removing a stale installation lock
and rerunning the intended tested candidate. Never remove another active
writer's lock. Runtime mutation uses a separate kernel-backed file lock.

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

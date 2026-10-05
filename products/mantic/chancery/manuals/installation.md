# Mantic installation and private state

Mantic is a short-lived local Rust CLI. It has no daemon, LaunchAgent, automatic
schedule, credential, model, or network integration. Installation selects one
retained program archive and its matching provider bundle. Definitions live
outside that release in `~/Library/Application Support/Mantic/mantic.db`.

## Installation interfaces

```text
./ci.sh submit COMMIT --deploy mantic
mantic-install inspect [--home ABSOLUTE_PATH]
mantic-install recover --release ABSOLUTE_PATH [--home ABSOLUTE_PATH] [--expected-current absent|releases/ID]
mantic-install deploy < REQUEST.json
```

Run CI from the Cell root and verify the retained Telete job and deployment
outcome. CI and Telete are the sole Cell deployment route. The product-owned
`deploy` command is the manifest instruction that Telete invokes.
Use `mantic.install.operate` for the supported procedure. The deployment request
on stdin uses schema two. Mantic accepts an empty settings object and has no
deployment settings. Deployment stages the supplied matched program and provider bytes,
selects them, and initializes only empty or already supported default private
state through Mantic's store. It adds no configs or expense items.

Direct `install` and `recover` select programs and documentation only. They do
not initialize, migrate, restore, or remove definitions. `inspect` reports
current release selection; it does not prove database validity or forecasting
success. A `--home` override isolates installation paths and does not become a
retained runtime database override.

## Release selection and ownership

The installer retains releases beneath
`~/Library/Application Support/Mantic/install/releases/UUID`. Each release
contains its programs, recorded file inventory and modes, and provider bundle
under `share/chancery/mantic`. It copies selected files into the fixed
`~/Library/Application Support/Mantic/install/runtime/` tree. Runtime executables
are regular files; update and recovery replace them at the same actual paths.
Each file replacement is atomic; the complete tree is not one atomic update.
`current` records the selected archive and `previous` retains the prior archive.
The installer owns
`~/.local/bin/mantic` and `~/.local/bin/mantic-install`.

The single Mantic provider selector is
`~/Library/Application Support/Chancery/providers/mantic`. It follows Mantic's
current retained archive. Installation rejects a pre-existing selector owned by something
else. Product and provider bytes remain useful when the Chancery reader is absent.
Mantic does not call the catalog to execute a forecast or edit a config.

An optional `--expected-current` guards the prior selection. `absent` requires
no selected release; `releases/ID` requires that exact prior selection. A mismatch
fails instead of replacing unexpected selection. Explicit recovery selects the
supplied retained compatible release and its matching provider together.

A failed instruction can leave completed publication changes in place. Inspect
the retained selection and failure before deciding on recovery. Installation
does not automatically restore the old program or delete private definitions.
It does not audit native signatures or retained file integrity as a gate.

## State and compatibility

Persistent schema one contains current config and expense definitions only.
`mantic init` initializes empty state and preserves supported existing state.
Ordinary config and forecast commands require initialized supported state.
They do not initialize, migrate, or repair it. Forecast reads are read-only.

Direct database integration and restoring configs through program rollback are
unsupported. Do not run an incompatible program against state or replace a
populated database to overcome a version error. This initial release supports
schema one; no older persistent schema migration or future migration window is
promised. Forecast CLI output uses schema two; config and item output remains
schema one. These output schemas and entry contract versions have separate
meanings. Forecast schema-one parsers must update for tagged occurrence sources
and complete effective and excluded expense definitions.

Names, amounts, schedules, and output remain private local data. Initialization
creates private parent directories and state. Filesystem access by the local
user is the trust boundary. Programs and provider bundles contain no private
configuration or forecast output. Installation neither sends nor retains a
forecast. No release-count or installation-latency objective is promised.

## Separate introductions

Run `mantic --register-usage` after selecting a new program. This separately
initializes only empty supported Chancery usage state and registers the declared
command inventory. It does no Mantic work. Usage records contain command identity,
time, and thread correlation, with no arguments, outputs, or outcome. Recording
errors preserve product command results; unsupported journal state stops explicit
registration. Read `chancery.usage.record` for that separate retention boundary.

Semantics registration introduces Mantic's project identity and maintained
terminology. It does not initialize Mantic state, install a program, establish
catalog readiness, or provide runtime calculation authority.

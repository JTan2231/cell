# Install and recover Annals

Annals uses local SQLite libraries and an optional filesystem inbox. Nucleus
provides model execution and authentication. On macOS, Clockwork starts the
inbox worker for the current user.

- [Configure paths and limits](#configuration).
- [Deploy or update on macOS](#deploy-or-update).
- [Provision a decisions library](#provision-the-dedicated-decisions-library).
- [Recover authentication or installation](#recovery).
- Use [Linux installation](linux-installation.md) for systemd.
- Use [older-installation migration](migration.md) for a pre-version-3 library
  or a root-owned macOS installation.

## State and prerequisites

Deploy and authenticate Nucleus before Annals. Deploy Clockwork before enabling
a macOS inbox. The services run with the current user's authority.

The macOS state root is `~/Library/Application Support/Annals`. It contains
`config.toml`, `usage.toml`, the primary `annals.db`, `spool/`, `log/`,
`backups/`, and immutable program releases under `install/releases/`.
The commands `~/.local/bin/annals` and `annals-usage` select the current release.
Their Chancery providers follow the same selection.

Named libraries use `ANNALS_STATE_DIR/catalog.db` and
`libraries/LIBRARY_ID/{annals.db,config.toml,spool/}`. The default state root is
`~/.local/share/annals` outside macOS. Each registration has a stable library
ID and pins that identity in its config. Creating a library starts no model or
schedule. Repeat an interrupted `library create NAME` to complete it.
Existing operator-selected paths are not registered automatically.

Each database stores its instruction history and current selection. A backup
preserves these settings with the source and corpus history. An instruction
change does not reinterpret existing records.

## Configuration

The packaged examples are [Annals config](../packaging/systemd/annals.toml)
and [Usage config](../packaging/systemd/usage.toml). For example:

```toml
library = "/absolute/library/annals.db"

[inbox]
root = "/absolute/library/spool"
settle_seconds = 60
minimum_available_bytes = 7_000_000_000

[liaison]
quality = "high"
nucleus_socket = "/absolute/nucleus/nucleus.sock"
```

Annals Usage has a separate config:

```toml
nucleus = "/absolute/bin/nucleus"
nucleus_socket = "/absolute/nucleus/nucleus.sock"
library = "/absolute/library/annals.db"
spool = "/absolute/library/spool"
```

Both configs must select the same reachable Nucleus socket. Usage uses the
`nucleus` executable only for delegated login. It calculates reports from live
Nucleus output and Annals records; it stores no reporting database.

The CLI selects `--config`, then `ANNALS_CONFIG`. Library selection uses
`--library`, then `ANNALS_LIBRARY`, then the selected config. The installed
macOS frontend supplies the primary config only when neither is explicit.
There is no current-directory fallback. Config-relative library and spool
paths resolve from the config directory. CLI and environment paths resolve
from the working directory. Unknown config keys are rejected.

Usage selects `--config`, then `ANNALS_USAGE_CONFIG`, then `usage.toml` beside
`ANNALS_CONFIG`, then the macOS default. Its relative paths use its own config
directory. See [usage reporting](telemetry.md).

`settle_seconds` defaults to 60; zero makes a source immediately eligible.
The storage reserve defaults to 7,000,000,000 bytes on each library and spool
filesystem. Zero disables it. This is a check before a claim, not a disk quota.
An active job or another process can consume space after the check.

`quality` defaults to `high`. An explicit `model` changes the model while the
quality preset selects reasoning effort. See [integration presets](cli.md#model-assisted-integration).
A liaison has a 60-minute timeout. An inbox activation has no item or lifetime
limit and can continue while work arrives.

## Deploy or update

From the Annals directory, use the prepared executables:

```sh
../target/release/annals-install install \
  --binary "$PWD/../target/release/annals" \
  --usage-binary "$PWD/../target/release/annals-usage" \
  --bundle "$PWD/chancery/annals" \
  --usage-bundle "$PWD/chancery/annals-usage" \
  --nucleus "$HOME/.local/bin/nucleus" \
  --nucleus-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock" \
  --clockwork "$HOME/.local/bin/clockwork"
```

The installer stages an immutable release, acquires the product lock, and
checks the prior installation. It holds command admission and spool mutation,
then lets active work finish. It backs up supported libraries before migration.
It restores compatible database state before restoring public commands after a
failed update.

Schema 6 accepts additive migration from schemas 3 through 5. It preserves
sources, deliveries, corpus history, admission kinds, and spools. It seeds
library instructions and leaves older examination provenance null. Registered
libraries receive separate identity checks, backups, and migrations. The
installer does not add schedules for those libraries.

The primary Clockwork binding is `annals/inbox`. It selects a hashed native
runner in the immutable release. The runner executes the sibling payload with
`--quiet inbox run` and umask `077`. Its scrubbed environment contains only
`HOME`, `USER`, `LOGNAME`, and `ANNALS_CONFIG`.

The definition requests run-at-load and a 300-second interval. It skips overlap
and has no activation timeout. The macOS GUI session must be available.
Clockwork records process outcomes; Annals records delivery and corpus results.

Updates preserve operator pauses. Coordinated deployment also preserves the
prior enabled state of owned schedules. Annals compares the complete selected
definition or legacy plist before changing it. Foreign or uninspectable state
stops the handoff. Do not change the same binding concurrently.

The default inbox-lock wait is 3,900 seconds. Set `ANNALS_UPDATE_WAIT_SECONDS`
to another nonnegative value if needed. This is not an overall deployment
timeout: Clockwork disable can wait for a no-timeout child to finish.
`--no-start` verifies an installation without changing scheduler state; it does
not complete a scheduled installation.

## Provision the dedicated decisions library

Krisis accounts use a separate `decisions` library. The primary installer does
not activate it. The Cell adapter composes primary deployment and this provisioner.

Invoke the installer from an exact immutable release:

```sh
release="$HOME/Library/Application Support/Annals/install/releases/<64-hex-release-id>"
"$release/bin/annals-install" provision-decisions \
  --release-root "$release" \
  --nucleus-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock" \
  --clockwork /Users/joey/.local/bin/clockwork
```

`--release-root` must name the content-addressed directory, not `current`.
The invoked installer must match its hashed release member. `--home` selects
an explicit alternate home.

The provisioner owns only `Annals/decisions/` and `annals/decisions-inbox`.
It shares the primary update lock. It stages fresh state or backs up and
migrates existing state under maintenance. It registers the candidate inactive,
drains an enabled owned prior binding, verifies readiness, and selects the
exact digest. It does not change `annals/inbox` or Nucleus.

The decisions root contains `config.toml`, `annals.db`, `spool/`, `log/`, and
`backups/`. The config, spool binding, and database must identify the same
immutable decisions library. A general library is rejected. Existing mutable
files must be private, operator-owned regular files with one hard link.
Stdout and stderr logs must be distinct `0600` files.

The binding uses a 300-second interval with run-at-load. Only validated
`inbox accept --producer krisis` calls admit original jobs. Generic retention,
integration, registration, enqueue, and backlog import are rejected. Scheduled
runs leave `incoming/` untouched. See [account exchange](../chancery/annals/manuals/decision-account-exchange.md).

Success reports config, library identity, Clockwork key and digest, selected
and enabled state, maintenance state, release ID, and `contract_version: 1`.
`--keep-maintenance` retains an owned hold for an outer cutover. A later
successful invocation without it releases that hold. An unrelated existing
maintenance marker is preserved.

A pre-commit failure restores captured state and the prior binding, including
its enabled state. Unproved restoration retains maintenance and the transaction.
Only an attributable candidate can be disabled during recovery.

## Recovery

### Authentication

Pause dispatch and wait for the active delivery to finish. Then renew and check
Nucleus-owned authentication:

```sh
annals inbox pause
annals inbox status
annals-usage login --device-auth
annals-usage doctor
```

Resume only a pause created for this recovery. If jobs failed before the outage
was detected, keep the pause and use [bounded retry](inbox.md#retry-failed-deliveries).
A failed account preflight leaves queued jobs unattempted.

### Interrupted installation

The installer retains `install/transaction.primary.OWNER/journal.json` or
`install/transaction.decisions.OWNER/journal.json`. The journal contains prior
selection, configuration, scheduler state, and database recovery material.

```sh
"$HOME/.local/bin/annals-install" recover \
  "$HOME/Library/Application Support/Annals/install/transaction.primary.OWNER"
```

If public commands are suspended, invoke the retained candidate's exact
`bin/annals-install`. Recovery restores a pre-commit database through SQLite
backup or completes a committed handoff. A release selector alone cannot prove
database compatibility. Safe completion moves the journal to
`backups/deployments/` and preserves library and spool recovery material.
Nucleus credentials remain outside Annals rollback.

### Low storage and failed jobs

`inbox status` reports capacity at both checked filesystems. Low capacity leaves
the next job queued with no attempt or delivery. A later activation checks
again; no resume is required. `storage_probe_failed` requires correction of
the path or permission error.

The user must explicitly authorize any storage cleanup or reserve reduction
for the exact target and scope. A run, retry, update, or deployment request does
not authorize deletion, rotation, compression, movement, or replacement of data.

The reserve gates new inbox claims and separately checks explicit enqueue
copies. It does not gate manual integration or unrelated jobs and deployments.
Those operations can still fail from actual filesystem exhaustion.

Use [inbox recovery](inbox.md#crash-recovery) for failed or interrupted jobs.
Do not move failed envelopes back into the queue or edit their receipts.

## Coordinated maintenance

The coordinator's `apply` phase stages and verifies immutable release files.
`configure` runs the product-owned configuration, migration and selector
transaction with its schedule disabled. `verify` checks the installed result
without starting product work. `release` removes only the named admission hold.
After every affected hold is released, `activate` restores the captured enabled
state of the current selected definition. An originally disabled binding stays
disabled. Clockwork incident halts and product pauses remain in force.

Drain returns `waiting` while admitted commands or durable Nucleus jobs remain.
It neither cancels nor retries those jobs. A completely absent Nucleus
installation with no Nucleus database has no durable jobs to drain. An
unavailable existing runtime is not treated as an empty job inventory.

`annals-install adapter` provides the Cell coordinator interface. Each database
has a separate `<canonical-database>.cell-maintenance` gate. Holds block new
mutation while existing work settles. Controlled commands require the sole
matching `CELL_DEPLOYMENT_RUN_ID` and exclusive activity.

The installed executable must already support admission holds. A candidate
cannot prove that an older executable is held. Establish that installation
through its supported deployment procedure before coordinated updates.

Read-only feed access remains available. Verification checks library statistics,
inbox state, the decisions watermark, and Usage doctor. It creates no works,
reconciliations, or model jobs. Release removes only the coordinator's hold.
Unresolved installer transactions, unknown ownership, or incomplete drain keep
admission held. See [the installed operation contract](../chancery/annals/manuals/install-operate.md).

## Backup and retirement

Use `annals backup OUTPUT` for a consistent SQLite copy. Include the spool when
recovery must preserve queued work, retry children, priorities, and sequence.
Annals retains terminal source archives until an explicit retention decision.

Retirement must prove ownership of the selected definition, any legacy plist,
and every command and provider selector. There is no supported path-only
removal sequence. Keep the installation intact until a product-owned operation
establishes those identities. Retained state, versioned release bytes, and
Clockwork history require their own retention decisions.

## Scheduled failure policy

Both `annals/inbox` and `annals/decisions-inbox` use Clockwork definition schema
2 with `[failure] on_abend = "halt-until-approved"`. The release-local runner
selects `inbox run --stop-on-failure`. This batch option stops after its first
failed job, including an item-local source failure, and returns nonzero before
claiming a successor. It does not create an Annals scheduling-pause record.
Ordinary manual `inbox run` retains its item-local continuation behavior.

Clockwork retains the incident, halts later activations, and queues one email
notification through `HOME/.local/bin/email`. Inspect `clockwork incident list
annals/inbox` or the `annals/decisions-inbox` key and `clockwork incident show
INCIDENT_ID`. Only explicit approval followed by `clockwork binding resume KEY
INCIDENT_ID` releases that scheduling halt. Definition switches, deployment,
Annals `inbox resume`, and dependency recovery do not release it.

A low-storage readiness result, operator pause, maintenance, or empty queue is
not an abend. A storage-probe or authentication error is an abend. Annals retains
operator pauses, bounded retry-event halts, exact attempts, and domain recovery.
Scheduling continuation neither retries a failed delivery nor clears these
product controls. Existing failed archives are history, not new incidents.

Deployment settings accept only `enabled`. `enabled` must be a boolean.
For example, `{"annals":{"enabled":false}}` keeps the candidate schedule
disabled after group activation. This setting applies to both installer-owned inbox bindings. An omitted value preserves
captured intent; a new schedule defaults to enabled. Recovery to the prior
configuration preserves captured intent and ignores this override. Incident
halts and operator pauses remain in force.

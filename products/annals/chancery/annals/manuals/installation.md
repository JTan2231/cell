# Annals installation and maintenance

Program publication copies the selected release into fixed regular files beneath
~/Library/Application Support/Annals/install/runtime. Public commands use that runtime
tree; current and previous retain immutable UUID archive selections. Code signing and
runtime path identity are separate from release identity.

New Clockwork definitions use schema 3: they retain the archive release ID, root and
exact hashes, and execute the fixed runtime image. Publication precedes registration.

Before publication, deployment runs the disable transition for owned Clockwork bindings
and waits for their active processes, including an active manual run on a disabled
binding. It restores saved enabled intent after registration; a failed instruction can
leave the owned bindings disabled.

Existing history, delivery records, enabled intent and incident halts retain their
meaning. Retained definitions and wrapper bytes from before this change keep their
legacy execution paths until a new installation or definition selects the runtime image.

Decisions provisioning requires the currently selected Annals archive because both inbox
schedules share its fixed runtime image. Manual primary installation requires
annals/decisions-inbox disabled because its separate provisioner owns the schedule
handoff. With --no-start, annals/inbox must also be disabled. The installer rejects
enabled bindings without a handoff and waits for active processes in retained disabled
bindings before runtime publication. A later explicit schedule handoff selects the
matching definition.

Initialization, migration, and recovery leave readable persistent WAL coordination files for readers without write access. Named creation prepares them at the final library path. Migration prepares the configured spool control lock. Maintenance status opens existing private locks read-only and never creates or repairs a gate. Missing required coordination state stops inspection until an authorized setup or recovery operation prepares it.

The user-owned macOS deployment installs Annals and Annals Usage together,
plus configuration, immutable releases, and the scheduled inbox
Clockwork binding `annals/inbox`. Nucleus remains a separately installed
execution and credential service; Clockwork remains a separately installed
activation and process-history service.

The explicit manual installer also discovers registered named libraries in Annals'
`catalog.db`. It records identity and paths, fences new command admission,
drains admitted work, and performs transactional schema migrations in place.
Program recovery preserves current data and requires compatible prior programs.
Supported migration preserves source, graph, admission kind, spool, and instruction
history. Read `annals.libraries` for schema and legacy instruction provenance. Registered names are not evidence of runtime readiness.

New libraries use `annals library create NAME`. Creation and ordinary
installation add no named-library schedules and do not automatically register
existing primary or decisions paths as names. The decisions provisioner retains
its separate admission and binding authority.

## Release selection and recovery

The explicit manual installer stages a immutable release without artifact or dependency
readiness checks. It starts Annals maintenance, drains
scheduled work, and performs supported migration. It then switches the release
and exact Clockwork definition digest, and publishes the installed commands.
The definition is registered inactive after runtime publication. Before the
deployer disables or replaces a binding, it reads current release metadata and compares every stored
executable-definition field with it. A foreign definition with the same key
stays untouched. The first handoff similarly removes only an exactly owned
legacy LaunchAgent. It does not stop, replace, or take ownership of Nucleus or
Clockwork.

Definition inspection and binding mutation are not a compare-and-swap.
Concurrent same-user direct Clockwork mutation of `annals/inbox` during deploy
or migration is unsupported; reinspection detects attributable changes where
possible, fails the handoff closed, and may retain maintenance for recovery.

The immutable definition requests run-at-load and a 300-second interval. It
skips overlap and has no activation timeout. It pins the native Rust Annals runner by SHA-256 and executes it from
`install/runtime/bin`. The runner executes its sibling runtime payload as `annals --quiet inbox run --stop-on-failure` in the Annals state directory
with an explicit nonsecret environment and umask `077`. Clockwork records
process outcomes but does not inspect Annals domain state or ingest
Annals-owned log bodies.

The inbox storage gate is not a deployment lock. A closed gate does not by
itself reject an Annals deployment, globally stop the Nucleus service or
independent Nucleus jobs, or block another product's deployment. The deployer
still needs enough physical capacity to stage the release and write migration and control artifacts; any of those writes can fail
when the shared filesystem is actually full. A probe error is distinct from a
measured closed gate and can fail deployment status inspection with
`storage_probe_failed`. A deployment request authorizes only the deployer's
documented effects. It does not authorize a model or agent to clear user data
as separate storage remediation or to lower or disable the reserve. Either
action remains a user decision requiring explicit consent for the exact target
and scope.

An ordinary pre-commit failure preserves current library and spool data.
It restores compatible program and configuration selections, then restores
the exact prior Clockwork definition only if
its binding was enabled, or restores the legacy LaunchAgent, never both. A
previously absent or disabled binding stays disabled without transient
activation; its inactive selected digest may remain the candidate digest. If
that exclusive restoration cannot be proved, Annals leaves unproved scheduler
state untouched, keeps maintenance, removes its public selectors, and retains
private recovery material. Do not remove maintenance markers, edit
receipts, or swap database files manually after interruption. Run the exact retained
`annals-install recover TRANSACTION_PATH`; it proves the product journal and
release evidence before restoring pre-commit state or completing a committed
operation. Completed control journals are retained in `install/transactions/`.
No data backup or restoration is supplied. Older backup files remain unchanged.

The attended migration from the former system LaunchDaemon uses a narrower
handoff. It can restore the system installation only before child installation
begins. Phase `installing`, or `rewritten` with child evidence, retains moved state
and outer and child recovery evidence instead of moving state back. Its child
in-place deploy keeps Annals maintenance in place and renders the exact Clockwork
definition, but does not register or select it.
The outer migration verifies that inert file and durably records its committed phase
before registration or binding selection, so the definition never points at a
state root that rollback can move away. RunAtLoad remains maintenance-gated
while the outer migration registers the definition, records its digest,
selects `annals/inbox`, and retires the system files. It clears maintenance
only after `system/org.annals.inbox` is proved absent and those steps complete.
A failed bootout or still-loaded service retains the legacy files,
transaction, and maintenance marker. A committed interruption retains the
transaction and handoff so a rerun can finish the same definition and binding
idempotently. Before commit the migration accepts only an absent Clockwork
binding or a disabled tombstone with no selected digest. Legacy plist removal
and restoration additionally require the complete file to match Annals'
rendered template, expected owner, and mode. A familiar label or executable is
not ownership, and an extra launchd key is treated as foreign.

## Dedicated decisions installation

The Rust provisioner uses release metadata and compiled configuration and
definition rendering. It does not compare its executing bytes with the retained
installer or audit the release inventory.

This authorizes creation or supported migration only under
`$HOME/Library/Application Support/Annals/decisions` and registration or
switching only of `annals/decisions-inbox`. It does not deploy release bytes,
change Nucleus, or inspect or mutate the primary `annals/inbox` binding. It
shares Annals' product-wide `install/.update-lock`, so it cannot race the
primary deployer.

The provisioner reads release metadata and attributes the selected prior definition.
It creates and binds fresh state outside live paths, then starts maintenance
before a run-at-load definition can be selected. It registers the candidate
inactive, drains an enabled owned prior binding, and migrates existing state
transactionally in place. It switches the exact definition after setup and migration.

Fresh state has the immutable `decisions` role. Ordinary Annals commands retain their library-role checks. Foreign or
concurrently changed state stops the operation. A pre-commit failure preserves current data and restores compatible program,
configuration and prior schedule selection and enabled state. A previously
disabled schedule stays disabled throughout recovery. If exact restoration
cannot be proved, the library retains maintenance. The provisioner disables
only an attributable candidate and reports retained transaction material for
recovery.

Before opening existing state, the provisioner requires its config, database
and SQLite sidecars, spool identity and control files, and maintenance files to
be operator-owned `0600` regular files with one hard link. It creates or
validates distinct private stdout and stderr log files before selection. A
symbolic link, extra hard link, foreign owner, or broader mode fails closed.

Success emits a single JSON envelope whose `data` contains contract version,
absolute config path, persistent library ID, Clockwork key and definition
digest, selected/enabled booleans, maintenance state, and release ID. With
`--keep-maintenance`, an Annals-owned receipt leaves the decisions gate engaged
for an outer Krisis/Semantics cutover. A later successful invocation without
that option clears only that matching owned hold. A pre-existing unreceipted
maintenance gate remains engaged.

## Deployment admission and coordination

The Cell manifest runs one `annals-install deploy` command. The schema-2
stdin request supplies candidate and source paths, the run identity and product
settings. Annals performs its own primary setup and dedicated decisions
provisioning. It creates or migrates libraries, writes configuration, places the release, and directly selects the two schedules. It
does not hold or drain durable application work or check readiness,
or recover automatically. Shared deployment does not run a mandatory application lifecycle
or parse Annals state as its success condition.

Annals preserves each binding's enabled state unless the optional boolean
`enabled` setting supplies an override. A new binding defaults enabled.
Clockwork incident halts and product pauses remain in force. Annals retains
its own migration and explicit transaction-recovery procedures.

```text
annals --library DATABASE --json maintenance status
annals --library DATABASE --json maintenance hold RUN_ID
annals --library DATABASE --json maintenance release RUN_ID
```

These commands use the private sibling `<database>.cell-maintenance` without
opening, initializing, or migrating the database. Status leaves an absent
gate absent. Its standard `ok/data` envelope contains `protocol_version: 1`,
`contract_version: 1`, all `holds`, and `drained`. Drain describes live command
admission. Annals owns any additional waiting needed for its setup commands.

Holds atomically fence new mutations and survive process exit. Existing
commands may finish. Hold and release are idempotent, and release removes
only its named owner. Read-only corpus/feed commands and inbox pause or
interrupt remain available. Operator pause is never cleared by deployment.
Run IDs have 1–128 ASCII letters, digits, hyphens, underscores, or periods and
cannot begin with a period. Invalid IDs or unprovable admission fail closed.

A controlled installer command can set `CELL_DEPLOYMENT_RUN_ID` only for the
same sole hold and exclusive drained activity; with no hold, the command uses
ordinary admission. Annals Usage doctor can prove intentionally held Nucleus
readiness for this same owner through the typed deployment-health interface;
that proof does not enable normal Nucleus job submission.

`annals-install deploy` performs primary and decisions setup as one product-owned
command. Existing application maintenance is preserved; native database and
publication locks protect actual writes. An interrupted command can leave
completed effects in place. Inspect its log before a further operation. It does not perform native
signature audits or run binaries with `--version` or `--help` as installation
gates. Release labels come from the supplied provider descriptors. Explicit
operator diagnostics remain available separately.

## Data preservation

The installer migrates supported libraries in place and preserves current library
and spool data. It has no data backup, data restore, or reset mode. Unsupported
schemas stop deployment. Existing data backup artifacts remain unchanged.

## Verification, privacy, and retirement

Installed libraries, spools, existing prior data artifacts, logs, and Nucleus output
may contain complete private source and model context. Preserve private
ownership and permissions. Deployment does not authorize
`products/annals/release.sh`, deletion of prior recovery material, or a data reset.

There is no supported raw path-only retirement sequence. A shared Clockwork
key, launchd label, command pathname, or provider pathname is not ownership;
leave it intact unless a product-owned operation has proved the exact current
definition, fully rendered legacy plist, and selector targets before mutation.

Installation does not run library statistics, inbox status, Annals Usage doctor,
or an inbox maintenance smoke check. It reads library identity where setup needs
it, preserves operator pauses, and performs the requested initialization,
migration and publication. Runtime diagnostics remain separate.

## Configuration and limits

The macOS state root is `~/Library/Application Support/Annals`. It contains
`config.toml`, `usage.toml`, the primary `annals.db`, `spool/`, `log/`,
and immutable releases under `install/releases/`. Public commands
in `~/.local/bin/` use `install/runtime` files copied from the selected archive.
Both Chancery selectors follow the `current` archive.
Annals Usage is independently versioned and is installed with Annals. The joint
cutover preserves `usage.toml` and pins both configs to the selected Nucleus
socket. An obsolete `usage.db` and its sidecars are retained only inside the
uncommitted transaction for rollback and discarded after successful commit. Its
current configuration and live diagnostic semantics belong to
`annals-usage.execution.operate`.

The `cell-install-v4` release records the declared paths for both programs,
the exact installer, native frontend and runner roles, and both provider bundles.
Provider Markdown, entry JSON, and the schema-4 product overview are release
bytes. A documentation change follows the same selection and rollback as the
program. Product runtime does not invoke Chancery.

The default inbox-lock wait is 3,900 seconds. `ANNALS_UPDATE_WAIT_SECONDS`
accepts a nonnegative replacement. It is not an overall deployment timeout:
Clockwork disable can wait for its child, which has no activation timeout.
`--no-start` installs files and state without changing scheduler state. It does
not complete scheduled installation.

Annals installs only its own bindings and selectors. A closed inbox storage
gate is independent of deployment admission. There is no maximum installed
state size, deployment duration, retained-generation count, downtime, rollback
retention horizon, future support window, or deprecation period promised here.
The separately owned Nucleus runtime and authentication, Clockwork activation
and launchd availability remain external readiness conditions.

Read `annals.inbox` for queue and scheduled failure semantics,
`annals.libraries` for path/configuration selection and persistent library meaning,
and `annals.work.integrate` for the required Bazaar prompt selection.

## Linux service boundary

The packaged Linux route installs Annals and Annals Usage with systemd units,
a separate Nucleus service, explicit configs, and an `annals` service account.
It uses no macOS content-release transaction or Clockwork binding. The library
parent is writable for SQLite WAL coordination; the Nucleus socket is reachable
by the service account. The packaged default assumes Nucleus runs as `annals`
with `HOME=/var/lib/annals`. The packaged service sets
`ANNALS_USAGE_CONFIG=/etc/annals/usage.toml`. Authentication remains solely
Nucleus-owned.

The timer starts two minutes after boot and five minutes after the previous
service becomes inactive. `Type=oneshot` and the Annals inbox lock prevent
overlap. The service has no systemd start timeout because an examination can
last 60 minutes and a draining activation has no item or lifetime bound. An
unexpected processing failure ends that activation nonzero after archiving the
failed job. The packaged Linux unit does not provide Clockwork incident halts
or notifications. Read `annals.install.operate` for installation, checks,
source handoff, and maintenance.

## Command usage

After installation or update, register both command inventories with
`annals --register-usage` and `annals-usage --register-usage`. This starts no
product work. Recording requires a nonempty `CODEX_THREAD_ID` and stores command
identity, observation time, and thread ID in Chancery's private journal, without
arguments, output, or outcomes. Internal product calls are excluded; recording
errors preserve command results.

## Deployment settings

Deployment settings accept only `enabled`, which must be a boolean.
`{"annals":{"enabled":false}}` keeps both installer-owned inbox bindings
disabled after group activation. An omitted value preserves captured intent;
a new schedule defaults to enabled. Recovery to the prior configuration ignores
this override and preserves captured intent. Incident halts and operator pauses
remain in force.

No launchd availability or wake-up latency is promised beyond Clockwork's
observations. There is no migration horizon promised here. The packaged Linux
route relies on systemd activation and service-account/filesystem administration
without a dedicated installed contract; this gap remains visible in `resolve`.

# Krisis system installation

This document describes the packaged current-user macOS layout. Building a
candidate does not authorize release, deployment, hook trust, live migration,
or a live model job.

## Installed identities

- runtime executable and public command: `krisis`
- product-owned Rust installation command: `krisis-install`
- Chancery providers: `krisis` 0.4.0 and read-only compatibility provider
  `decisions` 0.4.0
- active Clockwork key: `krisis/observer`
- SQLite schema: 4
- Nucleus provider/requester: `krisis`
- account capture rule: `krisis/decision-account-classification/1`

For migration compatibility, persistent paths do not move:

- database and releases: `~/Library/Application Support/Decisions/`
- installed observer ownership receipt:
  `~/Library/Application Support/Decisions/install/krisis-observer-binding.txt`
- logs: `~/Library/Logs/Decisions/observer.stdout.log` and
  `observer.stderr.log`
- maintenance marker:
  `~/Library/Application Support/Decisions/.clockwork-maintenance`

The old public `decisions` command, `decisions/observer`, and
`decisions/daily-email` bindings are retired. Final cutover disables only an
enabled legacy binding whose selected definition is proven to belong to the
current Decisions release. Disabled or foreign legacy definitions are left
untouched. Retained legacy database rows and Clockwork history are not deleted.

## Dependency configuration

The observer needs exact absolute paths to Codex, Annals, and the dedicated
Annals decisions config, plus that library's persistent ID. The Codex path is
selected by the operator and recorded in the immutable Clockwork definition;
the observer never discovers a different installation at runtime. A packaged
prepare accepts:

```text
krisis-install install \
  --source-root /absolute/path/to/cell/decisions \
  --binary /absolute/path/to/krisis \
  --clockwork /absolute/path/to/clockwork \
  --codex /absolute/path/to/codex \
  --annals /absolute/path/to/annals \
  --annals-config "/absolute/path/to/Annals/decisions/config.toml" \
  --annals-library-id 0123456789abcdef0123456789abcdef
```

The library ID must be exactly lowercase 32-hex. Krisis passes the explicit
config to Annals; it never chooses a library by fallback or `--library`.

## Prepare and final cutover

Preparation is the default. It installs the content-addressed release,
registers and fully verifies its Clockwork definition, prepares private logs,
and deliberately leaves the maintenance marker in place. It does not change
the current release, command, provider, hook, database, baseline, or any
Clockwork binding.

The outer cutover operator must then prove the separately managed Annals and
semantic activation prerequisites. Krisis does not infer that proof from a
running Clockwork process. Only then repeat the exact command with
`--final-cutover`.

Final cutover validates every current selector, selected Clockwork definition,
target-bound observer ownership receipt, and legacy plist before mutation. It
disables only proven-owned enabled
schedules, suspends the old hook command for its timeout, proves SQLite
quiescence, and saves the database and sidecars. It runs the exact prepared payload through schema 4 and doctor in a scrubbed
environment before publishing the candidate command and providers.

Doctor uses the same explicitly selected Codex executable as the observer and
checks Conversations, exact Nucleus capabilities and requester contract, and:

```text
annals --config CONFIG --json decision-feed watermark
```

For an interactive `krisis doctor`, `observe process`, or `observe reconcile`,
set `CONVERSATIONS_CODEX` to that same path; the installed command does not
inherit Clockwork's observer environment. Doctor and process also require the
complete explicit Annals arguments documented in the CLI contract.

It accepts only the standard success envelope with contract version 1 and the
configured library ID. The baseline is created once during explicit final
cutover, before the exact Krisis hook and `krisis/observer` binding become
executable. The deployer rereads the active and legacy bindings after the
switch, proves the exact candidate is enabled and the legacy schedules are not,
then removes the maintenance marker.

The observer definition runs every 60 seconds with `run_at_load = false`, pins
the exact release-local runner and interpreter digest, records the selected
Codex executable as `CONVERSATIONS_CODEX`, and uses a scrubbed environment. The
scheduled wrapper refuses a missing, relative, symbolic, or non-executable
Codex path. It suppresses detailed child errors and emits only a fixed failure
message, so it writes body-free output to the existing Decisions log path.
Interactive `krisis` diagnostics remain detailed.

## Coordinated deployment maintenance

`krisis-install adapter OP` is the sealed Rust product boundary used by Cell's
deployment coordinator. It composes the product-owned prepare and final-cutover
lifecycle, retains product-owned maintenance through group verification, and
releases only the coordinator's named hold after verification. The separate
installer marker is authenticated by its own receipt and inode. It
preserves captured schedule enabled booleans and the write-once observer
baseline. An ordinary update never invents a legacy activation watermark or
performs an implicit Semantics cutover.

The adapter first proves that the installed public executable supports
`maintenance status`. An older installed executable cannot be fenced by a
candidate gate: coordinated inspection stops before effects. Bootstrap that
compatibility release through the existing documented deployer and its writer
quiescence procedure. Supported new installation still uses the product's
existing prepare and final-cutover path.

The CLI gate is the private sibling `<database>.cell-maintenance`, separate
from `.clockwork-maintenance` and its installer receipt. Every ordinary public
command is fenced before database access. The coordinator holds every affected
product before applying selected candidates. Controlled installation uses the
same sole `CELL_DEPLOYMENT_RUN_ID` and exclusive drained activity; no hold
means ordinary admission. Doctor can prove Nucleus readiness under this exact
run's Nucleus hold while ordinary model submissions remain stopped.

An ordinary Annals dependency update may change the exact Annals executable
or config pin while preserving the persistent decisions-library ID. Before
changing it, Krisis proves the selected prior definition against the prior
release and private ownership receipt's old binary, config, and library ID.
It then validates the newly requested target with candidate doctor. Matching
only the new paths does not prove ownership of the old definition. A changed
library ID, foreign receipt, or unproved prior definition stops the update;
this transition does not rebind durable account identity to another library.

Group release removes only the coordinator's named hold after product
verification. Recovery stops on a retained installer maintenance marker or
unfinished product transaction and requires the existing product recovery
procedure; it never removes such evidence to force progress. An unproved
installation remains held.

## Verification

After a separately authorized final cutover:

1. Run `krisis doctor` with the installed Annals configuration.
2. Run `krisis observe status` and confirm schema 4 and the write-once baseline.
3. Inspect `krisis/observer` definition, binding, runtime history, and body-free
   logs; confirm both retired Decisions keys are absent or disabled.
4. Inspect and explicitly trust the exact `~/.codex/hooks.json` definition.

An installed file or binding does not prove Codex hook delivery or Annals domain
success on its own.

## Recovery and uninstall

Pending Annals delivery is normal recoverable state. Repeated observer runs
submit the exact same producer key, bytes, config path, and library identity
until Annals returns `created` or `replayed`. Do not retry classification for
delivery failure.

Use `observe reconcile` for a missed hook, `observe retry` only after diagnosing
a terminal classification failure, and guarded `observe abandon` only after
proving one still-unbound source permanently unavailable. Never edit SQLite.

If deployment cannot prove exact selector restoration (including Clockwork's
inability to restore a selected definition to null), it disables the owned
candidate and retains the maintenance marker and private transaction backup for
explicit recovery. Uninstall matches the selected definition to the installed
ownership receipt and disables only that exact owned active binding; enabled
foreign or legacy bindings are left
untouched and stop the operation. It retains the database, baseline, receipt
ledger, legacy Decisions history, releases, logs, and Clockwork history.
Deleting those requires a separate destructive decision.

The deployment adapter verifies the installed dependency configuration with
doctor. Verification does not create observations or submit Nucleus jobs.

## Retained installation artifacts

The package builds and seals both `krisis` and `krisis-install`. The shared
`cell-install` Rust library verifies the complete `cell-install-v2` manifest,
artifact hashes and modes, both provider bundles, and current/previous/public
selectors. Krisis owns the hook, private state, dependency pins, scheduler
handoff, maintenance receipts, and database recovery. The static `krisis`
frontend and `krisis-observer` interpreter script remain release data because
the runtime contract pins those exact interpreted images.

Each release retains `bin/krisis-install` and the same executable at
`package/install`. `krisis-install verify-release ABSOLUTE_RELEASE` performs
read-only integrity verification. `krisis-install inspect [--home ABSOLUTE_HOME]`
checks the selected installation. Retained `package/install install` finds its
sibling package and provider data; pass the exact retained payload as `--binary`
and the same explicit dependency pins. Legacy formats 2, 3, and 4 remain exact
readable evidence for the supported Decisions-to-Krisis handoff. Archived shell
deployers do not understand the new manifest and are not the forward installer.

`krisis-install uninstall --clockwork ABSOLUTE_CLOCKWORK` detaches only owned
public selectors and the exact hook. Current/previous, releases, private state,
receipts, and the maintenance marker remain. Reinstallation uses the retained
Rust installer and its authenticated prepare/final-cutover flow. No command
infers safe database rollback from program versions or manifest identity.

After exact preparation, `--final-cutover --keep-maintenance` retains the
installer gate through external verification. Repeat the same candidate and
pins with `--release-maintenance` to prove the installed surfaces and remove
only that authenticated gate. `--expected-current absent|releases/HASH` is an
optional stale-selector guard. Controlled rollback keeps public commands
suspended until database restoration is proved. An uncertain publication is
first repaired to that suspended view, never to an executable old command over
candidate database state.

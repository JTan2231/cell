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

The library ID must contain exactly 32 lowercase hexadecimal characters. Krisis passes the explicit
config to Annals; it never chooses a library by fallback or `--library`.

## Prepare and final cutover

Preparation is the default. It installs the immutable release,
registers its Clockwork definition, prepares private logs,
and deliberately leaves the maintenance marker in place. It does not change
the current release, command, provider, hook, database, baseline, or any
Clockwork binding.

The outer cutover operator must then prove the separately managed Annals and
semantic activation prerequisites. Krisis does not infer that proof from a
running Clockwork process. Only then repeat the exact command with
`--final-cutover`.

Before mutation, final cutover validates every current selector, selected
Clockwork definition, observer ownership receipt bound to the target, and legacy
plist. It disables enabled schedules only after verifying ownership. It then
suspends the old hook command for its timeout, verifies SQLite is idle, and saves
the database and sidecars. In a scrubbed environment, it runs the prepared
payload through schema 6 migration and baseline activation before publishing the command and providers.

The separate runtime doctor uses the selected Codex executable and
checks Conversations, exact Nucleus capabilities and requester contract, and:

```text
annals --config CONFIG --json decision-feed watermark
```

For an interactive `krisis doctor`, `observe process`, or `observe reconcile`,
set `CONVERSATIONS_CODEX` to that same path; the installed command does not
inherit Clockwork's observer environment. Doctor and process also require the
complete explicit Annals arguments documented in the CLI contract.

The baseline is created once during explicit final
cutover, before the exact Krisis hook and `krisis/observer` binding become
executable. The deployer selects the active binding, retires owned legacy schedules,
and removes its maintenance marker.

The observer definition runs every 60 seconds with `run_at_load = false`, pins
the exact release-local runner and interpreter digest, records the selected
Codex executable as `CONVERSATIONS_CODEX`, and uses a scrubbed environment. The
scheduled wrapper refuses a missing, relative, symbolic, or non-executable
Codex path. It suppresses detailed child errors and emits only a fixed failure
message, so it writes body-free output to the existing Decisions log path.
Interactive `krisis` diagnostics remain detailed.

## Cell manifest command and explicit maintenance

The Cell manifest runs `krisis-install deploy` once with a schema-2 request on
stdin. It places release files, performs observer activation and migration,
writes the hook and Annals pin receipt, registers the observer definition, and
selects `krisis/observer` directly. Activation preserves an existing write-once
baseline. The command pins the installed Annals decisions library; it does not
choose a legacy Semantics activation watermark.

The command does not acquire application maintenance, drain live or durable
work, suspend scheduling, retire legacy schedules, check readiness, or recover
automatically. Native state and publication locks protect actual writes.
An interrupted command can leave completed effects in place. Inspect its
retained log, hook, receipt and current selection before a further operation.
Explicit manual final cutover and recovery keep their documented procedures.

Settings accept an optional `codex_bin` path and boolean `enabled`. Omission
preserves the existing Codex pin and enabled intent; a new schedule uses the
product's default Codex path and defaults enabled. Existing application
maintenance, product pauses and Clockwork incident halts remain in force.

Use explicit product admission commands when an authorized operation needs them:

```text
krisis --database DATABASE --json maintenance status
krisis --database DATABASE --json maintenance hold RUN_ID
krisis --database DATABASE --json maintenance release RUN_ID
```

The durable sibling `<database>.cell-maintenance` gate is separate from the
manual installer's marker. Hold fences public commands before database access.
Release removes only the named owner. IDs contain 1–128 ASCII letters, digits,
hyphens, underscores or periods and cannot begin with a period. The manifest
executor neither invokes these commands nor interprets their output.

The package includes `krisis` and `krisis-install`; the latter is retained as
`package/install`. Explicit manual installation takes exact payload and
Annals/Codex pins. `--final-cutover`, `--keep-maintenance` and
`--release-maintenance` remain separate manual operations. Uninstall retains
state, releases, receipts and history.

## Verification

After a separately authorized final cutover:

1. Run `krisis doctor` with the installed Annals configuration.
2. Confirm schema 6 in doctor and the write-once baseline in `krisis observe status`.
   Run `krisis health` after the observer has run to inspect activity and state age.
3. Inspect `krisis/observer` definition, binding, runtime history, and body-free
   logs; confirm both retired Decisions keys are absent or disabled.
4. Inspect and explicitly trust the exact `~/.codex/hooks.json` definition.

An installed file or binding does not prove Codex hook delivery or Annals domain
success on its own.

## Recovery and uninstall

A delivery error marks its observation failed on the first error and retains
the pending document. Explicit `observe retry` releases the same producer key,
bytes, config path, and library identity. It does not rerun classification.

Use `observe reconcile` for a missed hook, `observe retry` only after diagnosing
an observation failure, and guarded `observe abandon` only after
proving one still-unbound source permanently unavailable. Never edit SQLite.

If deployment cannot prove exact selector restoration (including Clockwork's
inability to restore a selected definition to null), it disables the owned
candidate and retains the maintenance marker and private transaction evidence for
explicit recovery. Uninstall matches the selected definition to the installed
ownership receipt and disables only that exact owned active binding; enabled
foreign or legacy bindings are left
untouched and stop the operation. It retains the database, baseline, receipt
ledger, legacy Decisions history, releases, logs, and Clockwork history.
Deleting those requires a separate destructive decision.

Forward recovery opens retained local state with the candidate's `observe status`
command before publishing its selectors. A valid observer baseline is required.
The manifest command does not invoke doctor. Runtime diagnosis is separate.

## Retained installation artifacts

The package builds and seals both `krisis` and `krisis-install`. The shared `cell-install` Rust library copies release files and publishes owned
selectors. It reads release metadata without auditing hashes, modes, provider
contents, or version alignment. Krisis owns the hook, private state, dependency pins, scheduler
handoff, maintenance receipts, and database recovery. The static `krisis`
frontend and `krisis-observer` interpreter script remain release data because
the runtime contract pins those exact interpreted images.

Each release retains `bin/krisis-install` and the same executable at
`package/install`. `krisis-install inspect [--home ABSOLUTE_HOME]`
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
only that authenticated gate. `--expected-current absent|releases/ID` is an
optional stale-selector guard. Controlled rollback keeps public commands
suspended while schema compatibility is proved. Program rollback preserves live
data and requires an unchanged schema. A schema change recovers forward with
the retained candidate. An uncertain publication is first repaired to the
suspended view.

## Scheduled failure policy

Krisis configures Clockwork definition schema 2 for `krisis/observer` with
`[failure] on_abend = "halt-until-approved"`. A conversation read failure
(`document_source_unavailable`), including a timeout or protocol error, is a
handled outcome after Krisis saves the failed observation. It returns zero,
permits later activations, and creates no pause alert. Other launch, dependency,
source-validation, classification, or Annals delivery failures end the current
run and begin a pending Clockwork failure episode. Later scheduled activations
remain admissible until the shared service-health threshold. By default, five
consecutive failed read-only checks, at least 60 seconds apart, halt the binding
and make its alert eligible together. Healthy or inactive checks clear a pending
episode. Failed observations still require explicit retry. A failed or cancelled
Nucleus job after an accepted classification preserves the classification and
reports that exact job to Clockwork; it creates no successor attempt. An empty poll or valid
maintenance gate is a successful no-work result. Krisis owns these outcome
meanings and its configuration; Clockwork owns the durable scheduling incident,
admission gate, and one retained email notification through
`HOME/.local/bin/email`.

Inspect `clockwork incident list krisis/observer` and `clockwork incident show
INCIDENT_ID`. After explicit approval, use `clockwork binding resume
krisis/observer INCIDENT_ID`. This allows later scheduling and does not retry a
failed observation. Use the separate guarded `observe retry OBSERVATION_ID` when
that recovery is authorized. It retains saved requests, accepted classifications,
exact documents, target identity, and idempotent Annals acceptance.

Definition switches and deployment preserve the Clockwork incident. Existing
failed observations remain terminal history; cutover does not re-alert or retry
them. The retired Decisions schedules remain disabled. Schema-one definitions
keep their old policy until a schema-two definition is explicitly selected.

Coordinated recovery restores the exact recorded product transaction before it releases its hold. Its private journal binds the deployment owner, home, prior
selection and candidate release to captured database, hook and schedule state.
It does not reclassify observations or run `observe activate` again. Evidence
from another owner or an older journal without that identity stays retained
for explicit recovery. Normal deployment preserves the existing Annals library
ID while updating its executable pin.

Deployment settings accept only `codex_bin` and `enabled`. `enabled` must be a boolean.
Set `codex_bin` to an absolute executable path to replace an existing reader pin.
Omit it to retain the installed pin. For the current ChatGPT app layout, use
`/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex`.
The installer records the replacement in the observer definition.
The observer continues to use only that recorded path.
An unavailable prior Codex executable does not prevent replacement when its
retained receipt and exact observer definition still prove ownership.

For example, `{"krisis":{"enabled":false}}` keeps the candidate schedule
disabled after group activation. An omitted value preserves
captured intent; a new schedule defaults to enabled. Recovery to the prior
configuration preserves captured intent and ignores this override. Incident
halts and operator pauses remain in force.

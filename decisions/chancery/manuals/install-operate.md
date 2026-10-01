# Install and operate Krisis

The public binary and provider are `krisis`. The compatibility provider is
`decisions`, the active Clockwork key is `krisis/observer`, and the schema is 6.
Existing Decisions application-support and log paths are retained for
compatibility with persistent history.

`krisis-install install` requires absolute paths for Krisis, Clockwork, Codex,
Annals, and the dedicated Annals config. The decisions-library ID must contain
exactly 32 lowercase hexadecimal characters. The selected Codex path is recorded
in the immutable Clockwork definition
and used unchanged for scheduled Conversations reads;
the observer does not discover another Codex installation at runtime.
Interactive source-reading commands must receive that same path through
`CONVERSATIONS_CODEX` because the installed command does not inherit
Clockwork's observer environment; doctor and process also require the complete
explicit Annals configuration. The default operation prepares a content-addressed release and Clockwork definition while retaining the
maintenance gate; it does not select or activate them. After the outer cutover
has separately proved its Annals and semantic prerequisites, `--final-cutover`
performs writer shutdown, quiescence, migration, baseline activation, selector/hook publication, and schedule handoff. Clockwork process state
is not cross-system proof.

Every selected definition and legacy plist is inspected and attributed before
mutation. Disabled or foreign legacy bindings are untouched. A provable failure
restores exact prior selectors and enabled state; a prior null Clockwork
selection that cannot be restored leaves the owned candidate disabled with the
maintenance gate and transaction evidence retained.

After a separately authorized final cutover, verify the baseline, exact hook trust, private observer ownership receipt, `krisis/observer` history,
retired binding state, and body-free logs. Uninstall disables only the
exact owned active binding and retains the database, receipt ledger, releases,
logs, scheduler history, and legacy history.

Installation does not validate persistent-state compatibility, audit release
integrity, or check dependency readiness. Its setup operations still create and
migrate state, maintain holds, and publish the requested
configuration. Ordinary product commands retain their runtime checks.

## Run-owned deployment admission

The coordinator's `apply` phase stages release files.
`configure` runs product configuration, migration and selector publication with
scheduling disabled. `release` removes only the named admission hold.
After every affected hold is released, `activate` restores the captured enabled
state of the current selected definition. An originally disabled binding stays
disabled. Clockwork incident halts and product pauses remain in force.

Drain returns `waiting` while admitted commands or durable Nucleus jobs remain.
It neither cancels nor retries those jobs. A completely absent Nucleus
installation with no Nucleus database has no durable jobs to drain. An
unavailable existing runtime is not treated as an empty job inventory.

```text
krisis --database DATABASE --json maintenance status
krisis --database DATABASE --json maintenance hold RUN_ID
krisis --database DATABASE --json maintenance release RUN_ID
```

The private sibling `<database>.cell-maintenance` is separate from the
installer's `.clockwork-maintenance` marker and receipt. These commands never
open, initialize, or migrate SQLite; status leaves an absent gate absent.
Their JSON has `protocol_version: 1`, `contract_version: 1`, `holds`, and
`drained`. Drain describes participating live commands. The product must
separately verify that durable observations and dependency jobs have stopped.

Any hold fences every other public CLI and typed client command before
database access, including status and doctor, since opening state can migrate
it. Existing commands may settle. Holds survive process exit; repeated hold
and release are idempotent. Release removes only its named owner and preserves
the observer baseline. IDs contain 1–128 ASCII letters, digits, hyphens,
underscores, or periods and cannot begin with a period.

Controlled commands set `CELL_DEPLOYMENT_RUN_ID` for the exact sole hold and
exclusive drained activity. With no hold they use ordinary admission. Only
doctor can use this identity to prove deliberately held Nucleus readiness;
runtime drain, authentication, harness, product capability, and protocol
checks still apply. Observation processing requires normal Nucleus admission.

The sealed Rust `krisis-install adapter OP` boundary composes preparation and explicit final cutover while
preserving captured schedule enabled booleans and baseline identity. It does
not infer a legacy Semantics activation watermark. An ordinary Annals binary
or config pin update proves the prior definition against its release and old
receipt target, requires the same persistent decisions-library ID, then records the new target. A foreign receipt or changed
library ID stops the transition.

Coordinated inspection requires maintenance support from installed public
executables before effects. Unsupported old binaries need their compatibility
release through the documented deployer and writer-quiescence procedure; a
candidate gate cannot fence them. Recovery stops on retained installer
maintenance or an unfinished product transaction and leaves the outer hold
for the existing recovery procedure. It never deletes those markers or
another owner's hold to force progress.

Installation and coordinated deployment do not run doctor or audit artifact
bytes. They prepare the requested files, configuration, migration and activation
baseline under maintenance. Runtime doctor remains available separately.

The package builds both `krisis` and `krisis-install`. New releases retain the
exact Rust helper at `bin/krisis-install` and `package/install`, with a complete
`cell-install-v2` artifact manifest. Static frontend and observer scripts remain
release data. The shared Rust library copies artifacts and owns selector transactions; Krisis owns hook, database, admission, and scheduler lifecycle.
`krisis-install inspect` checks the selected installation. Retained
`package/install install` uses its sibling package data and explicit exact
payload/dependency pins. Source invocation supplies `--source-root` for the
absolute Decisions product source directory. Legacy formats 2, 3, and 4 remain
readable migration inputs; archived shell deployers do not install the new
manifest. Uninstall uses `krisis-install uninstall --clockwork ABSOLUTE_PATH` and
retains current/previous, releases, private state, receipts, and maintenance.

For a source build, prepare with:

```text
krisis-install install --source-root ABSOLUTE_DECISIONS_SOURCE --binary ABSOLUTE_KRISIS --clockwork ABSOLUTE_CLOCKWORK --codex ABSOLUTE_CODEX --annals ABSOLUTE_ANNALS --annals-config ABSOLUTE_CONFIG --annals-library-id LOWERCASE_32_HEX
```

After the separate cutover prerequisites are proved, repeat the same inputs
with `--final-cutover`. Add `--keep-maintenance` only after a successful exact
preparation to retain its authenticated gate through external verification.
Repeat the same inputs with `--release-maintenance` to release that gate after
proving the exact current command, providers, hook, receipt, enabled observer,
and retired legacy schedules. `--home` selects an absolute operator home;
`--expected-current absent|releases/HASH` optionally refuses a changed selector.
The marker and receipt are distinct from the coordinator's named CLI hold.

## Inspect worker operation

Run `krisis health [--max-idle-seconds N] [--json]` for current worker health.
The command uses local worker records and the serial processing lock, needs no
Annals configuration, and obeys maintenance admission. It can migrate state.
`working` means the lock has a live owner; `idle` means the last run finished
within the selected limit. The default limit is 180 seconds for the installed
60-second schedule. Empty polls preserve continuous idle time. This limit is a
local diagnostic threshold, not a Clockwork delivery guarantee.

`stale` means the last finished run exceeded the limit. `error` records an
unhandled worker error. `interrupted` means a started run has no finish and no
lock owner. `unobserved` means worker activity has not been recorded yet.
The JSON fields are `ok`, `state`, `checked_at`, `state_since`,
`state_duration_seconds`, `last_started_at`, `last_finished_at`,
`max_idle_seconds`, and `error_code`. Times are Unix seconds and durations are
seconds at the check. Unknown transition times and durations are null. Stale
state starts at last finish plus the limit; an interrupted exit time is unknown.
A working duration measures lock ownership, not classifier progress.
Working and idle exit zero; all other states print the report and exit nonzero.
Use `doctor` to check dependency readiness.

The first processing error marks its observation failed. A conversation read
failure (`document_source_unavailable`) returns zero after that record is saved
and leaves the worker idle. Other failures end the worker nonzero.
Historical failure counts do not determine health and need
no repeated alert. `observe status` reports their count for optional later review.
`observe retry OBSERVATION_ID` is explicit recovery. It preserves prior failures,
resumes uncertain saved jobs, and releases failed pending deliveries using their
same document key and bytes. No failed observation is selected automatically.

Schema 5-to-6 adds worker activity and failure history without requeuing work.
The migration copies currently failed observations into history and cannot
reconstruct older overwritten errors. Preserve the database and document runs
through quiescent migration. Program rollback preserves data and requires an
unchanged schema. Recover forward with the retained candidate after a schema change.

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
episode. Failed observations still require explicit retry. An empty poll or valid
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
selection and candidate release to the prior database schema, hook and schedule state.
Forward recovery opens and migrates retained local state through the exact
candidate's `observe status` command. It does not check dependency readiness.
An absent or invalid observer baseline stops recovery before hook publication
or maintenance release. Keep the exact transaction and maintenance. Explicitly
activate the observer through the retained candidate with the same deployment
owner, then retry recovery. Recovery does not choose or replay that baseline.
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

## Command usage

After each installation or update, run `krisis --register-usage`.
This registers command inventory without product work.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

## Bazaar prompt selection

Prompt preparation requires initialized private Bazaar state and a complete cell.prompts.krisis selection. The default database is ~/.local/share/bazaar/bazaar.sqlite3; callers accept an absolute CELL_BAZAAR_DATABASE override. Reads fail without creating state or using embedded fallback text.

Read `cell.prompts.krisis` with Bazaar's supported `get` interface. Its content
is `{"schema_version":1,"entries":{"PROMPT_ID":VERSION}}`, with every component
pinned to a positive integer version. Publish component text first, then publish
the complete selection. A text append alone does not change the selected set.
Missing or invalid selections stop new request preparation before model admission.

Import the migration seed before deploying these callers. Preserve selection
version 1 and all referenced text versions for compatibility. Runtime reads never
perform this import. Deployment does not supply missing prompt contents.

The caller freezes resolved instructions with the existing request or domain
snapshot. Retries retain that selection. Later edits do not rewrite saved work.
Models, permissions, schemas, tool execution, domain commits, and recovery remain
product-owned. Annals library instructions remain
immutable domain captures selected through their existing product operations.

For an edit, use `bazaar update PROMPT_ID --file /absolute/prompt.txt`, read the
returned version, and publish a complete selection with `bazaar update
cell.prompts.krisis --file /absolute/selection.json`. Use an explicit
`bazaar --database /absolute/private/bazaar.sqlite3` prefix when the caller uses
`CELL_BAZAAR_DATABASE`. To roll back, append the prior selection content. Keep
private text out of logs and retain historical versions.

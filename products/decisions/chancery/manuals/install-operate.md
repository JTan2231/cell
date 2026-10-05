# Install and operate Krisis

Program publication copies the selected release into fixed regular files beneath
~/Library/Application Support/Decisions/install/runtime. Public commands use that
runtime tree; current and previous retain immutable UUID archive selections. Code
signing and runtime path identity are separate from release identity.

New Clockwork definitions use schema 3: they retain the archive release ID, root and
exact hashes, and execute the fixed runtime image. Publication precedes registration.

Before publication, deployment runs the disable transition for owned Clockwork bindings
and waits for their active processes, including an active manual run on a disabled
binding. It restores saved enabled intent after registration; a failed instruction can
leave the owned bindings disabled.

Existing history, delivery records, enabled intent and incident halts retain their
meaning. Retained definitions and wrapper bytes from before this change keep their
legacy execution paths until a new installation or definition selects the runtime image.

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
explicit Annals configuration. The default operation prepares an immutable release and the exact Clockwork
definition digest while retaining the maintenance gate. It does not register,
select or activate that candidate. Final cutover publishes the runtime files
before it registers the definition. After the outer cutover
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

## Deploy through CI and operate explicit maintenance

Submit `./ci.sh submit COMMIT --deploy krisis` from the Cell root. CI and Telete
are the sole Cell deployment route. Add required products with another
`--deploy PRODUCT`. Use `--settings ABSOLUTE_JSON` for a private JSON file whose
`krisis` object contains explicit product settings. Telete freezes settings,
validates, prepares signed programs and executes the manifest. Verify its
retained job and deployment outcome.

The Cell manifest runs `krisis-install deploy` once with a schema-2 request on
stdin. It places release files, performs observer activation and migration,
writes the hook and Annals pin receipt, registers the observer definition, and
selects `krisis/observer` directly. Activation preserves an existing write-once
baseline. The command pins the installed Annals decisions library; it does not
choose a legacy Semantics activation watermark.

The command does not acquire application maintenance, drain durable
work, retire legacy schedules, check readiness, or recover
automatically. Native state and publication locks protect actual writes.
An interrupted command can leave completed effects in place. Inspect its
retained log, hook, receipt and current selection before a further operation.
Explicit legacy final cutover and recovery keep their documented procedures.

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
`package/install`. Explicit legacy bootstrap and recovery take exact payload and
Annals/Codex pins. `--final-cutover`, `--keep-maintenance` and
`--release-maintenance` remain separate manual operations. Uninstall retains
state, releases, receipts and history.

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

Krisis configures Clockwork definition schema 3 for `krisis/observer` with
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
keep their old policy until a schema-two or schema-three definition is explicitly selected.

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

Bazaar owns the shared selection format, loading, template rendering and trusted
reference expansion through `bazaar.prompts.prepare`. Krisis owns authored meaning,
selected components, runtime inputs and request assembly.

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

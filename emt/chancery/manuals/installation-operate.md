# Install and operate EMT

Use this operation to install matched EMT bytes, configure an installation,
activate its worker, inspect readiness, take a drained backup or recover a
coordinated update. EMT owns its configuration, correspondence, maintenance
holds and installation. Clockwork owns the worker schedule and failure halt;
Nucleus owns jobs; Email owns transport and credentials.

Read required features `emt.service`, `emt.incident.respond` and
`emt.quota-notices` for detailed behavior, authority, record meaning, deadlines
and recovery. This procedure keeps its operating prerequisites, consequential
effects, stop conditions and verification here. Reading it does not authorize
live jobs, account reads, messages, installation or activation.

## Prepare dependencies

1. Establish authorization for the selected action. Activation permits automatic
   investigation, account receiving reads and incident email. Agents have
   unrestricted current-user execution with Codex approval prompts disabled;
   recognized replies authorize one-off interventions and do not authenticate
   the sender. An older reply does not approve a newer halt.
2. Select compatible Email send, receiving and account interfaces, Nucleus with
   invocation policy version two and `workspace-unrestricted`, Clockwork incident
   feed and notification handoff, and Iatreion read-only service checks.
3. Review the prompt migration seed and select the intended private database.
   Run the explicit importer from the Cell checkout before deploying callers:

   ~~~sh
   cargo run --locked --offline --package cell-prompts -- \
     /absolute/private/bazaar.sqlite3 prompting/seed.json
   ~~~

   The importer initializes only that selected path and publishes selections
   after their components exist. Read `cell.prompts.emt` with Bazaar's `get`
   interface and verify all exact referenced versions. Preserve selection version
   1 and its components. Repeat the same import after interruption; inspect
   history after an uncertain write. Deployment and runtime reads create no
   missing text. Use an absolute `CELL_BAZAAR_DATABASE` override only when that
   database is also configured for the caller; an interactive override does not
   configure Clockwork's scheduled environment.
4. Resolve the receiving domain through Email `receive settings` before
   maintenance if none was supplied. Use an explicit domain when the account
   result is empty or ambiguous; do not inspect received mail to infer settings.
5. Select an existing stable absolute Cell root and absolute provider and agent
   paths. Refresh all active generated Clockwork broker plists to the compatible
   release before EMT routing. Stop if an old broker can run while claims exist.

Stop if authority, dependencies, prompt selection, receiving domain, ownership
or stable configuration cannot be established. Compatible installed documents
alone do not prove readiness.

## Install before initialization

1. Select separately authorized matched program, installer and provider bytes.
2. Run `emt-install install --binary ABS --bundle ABS`. Use shared `--home` and
   `--expected-current` options when needed to select and guard the installation.
3. Verify that the selected binary and provider release match. Run
   `emt --register-usage` to register command inventory without product work.

This installs bytes without initializing state, starting agents, sending mail
or enabling a schedule. Use the coordinated procedure below for initialized
updates; direct initialized selector recovery is unsupported.

## Initialize, configure and activate

1. Run `emt init` to create schema-one state and paused configuration.
2. Run `emt configure --receiving-domain DOMAIN --cell-root /absolute/cell`.
   Select `--agent-cwd`, `--model`, `--email-executable` or
   `--clockwork-executable` only as needed. Omitted values remain unchanged.
   Keep admission paused and work drained during configuration changes.
3. Run `emt doctor`. Verify local schema, SQLite check, Clockwork interface,
   Nucleus health and Email executable presence. Receiving permission and final
   delivery remain `not_probed`; stop if a required observation is unavailable.
4. Run `emt resume` after activation is authorized. This validates configuration,
   configures the Clockwork EMT route and opens admission. It does not enable
   scheduling or approve a failure halt.
5. Run `emt schedule enable`. Inspect `emt schedule status` and
   `emt --json status` to verify the intended schedule and admission states.

The worker may read accounts, start authorized agents and submit email. EMT
stores no credentials. Worker output uses counts and bounded waiting codes;
explicit exchange reads expose private correspondence. Keep state and backups private.

## Inspect, pause or recover a worker

1. Read `emt --json status`, `emt schedule status` and `emt maintenance status`.
   Treat counts as retained records, not lifetime totals or product health.
2. Run `emt pause` when discovery must stop. Admitted exchanges and frozen
   deliveries continue. Run `emt schedule disable` when worker scheduling must
   also stop; schedule state and admission pause are separate.
3. Inspect an unexpected worker failure through Clockwork and the owning product
   interfaces. Ordinary dependency unavailability can be waiting; a nonzero
   worker activation can create an exact `emt/worker` halt.
4. Resume an exact failure halt only when explicitly approved, with
   `clockwork binding resume emt/worker INCIDENT_ID`. Do not substitute
   `emt resume`, installation or a schedule switch for this approval.
5. Verify current schedule, pause, holds and halt evidence before reopening
   authorized admission with `emt resume` or enabling the schedule.

Preserve unknown outcomes. Inspect the saved exchange, Nucleus activity and
affected product state before repeating an external action. Do not create an
automatic replacement job or a new mail identity. Keep frozen quota-notice
records; deleting one to retry delivery is unsupported.

## Maintain and back up

1. Validate configuration and capture current pause, schedule and failure-halt
   intent before maintenance.
2. Run `emt maintenance hold OWNER` for the exact operation owner.
3. Run `emt maintenance drain` and inspect `emt maintenance status`. Drain
   advances existing exchanges without discovering new incidents or mail.
   Require `drained` and known outstanding counts; unknown is not zero.
4. Run `emt migrate --backup /absolute/private/emt-backup.sqlite3`. Select a
   destination separate from the live database and configuration. Preserve the
   database and companion `.config.json`; existing destinations must match and
   are never overwritten. With no database, migration initializes paused state
   and reports `backup:null`.
5. Preserve `quota-notifications/` and Clockwork's `failure-checks.json`,
   `notification-checks.json`, routing metadata and incident database with their
   private backups. Do not run an older broker while the new sidecar exists. EMT
   backups do not restore Nucleus or Email records.
6. Release only this operation's hold with `emt maintenance release OWNER`
   after verification. Recheck the captured operator intent and exact halt.

Stop on unsupported schema, failed backup verification, unknown drain,
unestablished hold ownership or unresolved external effects. Never release
another owner's hold or silently resume a halt. Nucleus restart cannot resume
old agent processes.

## Update and recover through the coordinator

1. Run the authorized initialized update through `./deploy.sh emt`. The
   coordinator captures configuration and worker intent, disables the worker,
   holds and drains EMT, records migration evidence, installs matched bytes,
   selects a disabled exact definition and verifies readiness before release.
2. Supply setup configuration or `enabled` and `paused` intent only when
   authorized. Omitted settings preserve saved values. Fresh valid deployment
   defaults to activation after verification; an existing absent binding remains
   absent unless activation is explicitly requested. Incoming-mail progress
   changes are not setup inputs.
3. Verify the retained coordinator migration receipt, completed schema and
   original backup digests. Confirm restored pause, enabled state and existing
   failure-halt evidence. A requester-only update leaves Nucleus admission open;
   Nucleus replacement waits for EMT exchanges before holding the service.
4. Run the next authorized ordinary `./deploy.sh emt` command after an
   interrupted phase to let the coordinator recover its retained transaction.
   Read `nucleus manual` for shared coordination. Reuse the original verified
   backup and captured configuration. Restore a disabled exact
   definition before release, then apply captured operator intent. Do not replace
   the original backup with already configured state.
5. Run `emt --register-usage` and verify matched binary and installed provider
   publication after installation or update.

Retain unresolved holds when recovery evidence cannot be established. Stop an
initialized direct-selector recovery attempt. Installation, initialization,
configuration, admission and scheduling remain separate effects.

## Verify the endpoint

Require the selected program and provider to match, configuration and schema to
be valid, maintenance counts to be known, and schedule, pause and halt intent to
match the authorized operation. Interpret each operation receipt separately.
Email acceptance proves submission to the provider; product evidence establishes
intervention success. Receiving authorization and final delivery require their
own evidence. No timer, model accuracy or final-delivery deadline is promised.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time and thread ID, not arguments, output or
outcomes. Internal product calls are excluded. Recording errors do not change
command results.

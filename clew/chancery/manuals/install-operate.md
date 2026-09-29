# Install and verify Clew

Use this operation to install, inspect, migrate, prepare a schedule, back up, or
recover Clew. Read `chancery resolve clew.install.operate` for the required
`clew.state`, `clew.application.track`, and
`clew.digest.email` contracts. They own the detailed behavior. This procedure
keeps prerequisites, effects, stop conditions, and verification in place.

Clew needs Cast read contract two for search, first-job admission, email context,
and deployment verification. A nonempty schema-one migration also needs Platter
read contract one. Email contract four supplies submission; Clockwork contract
three supplies activation. Clew runs no model requester.

Installation can select programs, initialize empty state, migrate legacy state,
and change explicitly supplied schedule intent. It adds no application reports
and performs no preparation or immediate send. Initialize and inspect commands
grant no mail or status authority.

## Deploy through the coordinator

1. Select a validated committed candidate on local main. Use
   `cell-ci submit COMMIT` for ordinary delivery. The manager integrates,
   validates, attempts bounded repairs, deploys, and emails the outcome.
2. Preview a separately authorized manual deployment with `./deploy.sh plan clew`.
   Confirm the selected product and dependency changes.
3. Set `daily_email_enabled` only when the intended schedule change is authorized.
   Omit it to preserve existing intent. Explicit true grants standing authority
   for the complete daily digest to Email's fixed personal recipient. Explicit
   false selects disabled intent.
4. Run `./deploy.sh clew`. The coordinator installs compatible dependencies when
   necessary, captures schedule intent, holds email admission, suspends the
   binding, and drains sends before configuration. A schema-one ledger uses the
   guarded migration below.
5. Confirm the retained deployment outcome. Verification must establish matching
   selected bytes, schema-two ledger integrity, and a successful complete Cast
   snapshot read. Confirm that activation preserved captured or explicit enabled
   intent and retained failure halts.

Stop for missing dependencies, unresolved or foreign holds, an incomplete drain,
invalid state, unsafe migration mappings, changed release bytes, or foreign
selectors. Preserve operation evidence for coordinator recovery. Do not release
holds, approve incidents, or force an older incompatible program to obtain a
successful result.

## Install program files directly

1. Select a sealed validated candidate and matching provider bundle. Confirm that
   any existing ledger is compatible. Direct installation does not migrate it.
2. Run the installer:

   ```sh
   clew-install install --binary /absolute/candidate/clew --bundle /absolute/clew/chancery
   ```

   Use `--home ABSOLUTE_PATH` for an explicit user home and
   `--expected-current absent|releases/HASH` when selection must match an exact
   prior condition. Stop if selectors belong to another owner or integrity fails.
3. Run `clew init` to create empty schema-two state or check compatible state.
   Stop for schema one, nonempty foreign state, or unsupported state.
4. Run `clew doctor`, `clew-install inspect`, and candidate verification:

   ```sh
   clew-install verify --binary /absolute/candidate/clew --bundle /absolute/clew/chancery
   ```

   Confirm ledger integrity and the selected release. Doctor does not probe Cast;
   confirm the required snapshot interface separately before first-job work.
5. Run `clew --register-usage` to register command identities without reports.

The default ledger is `~/.local/share/clew/ledger.sqlite3`. Use global
`--state-dir ABSOLUTE_PATH` to select another private ledger for Clew commands.
State directories require mode 0700 and databases require mode 0600. Keep all
private state outside source and release trees.

## Migrate a schema-one ledger

1. Use coordinated deployment. Prevent old schema-one writers from running and
   let admitted sends finish. Configure requires the sole run-owned email hold.
2. Confirm that every retained legacy Platter reference has one exact Cast job
   mapping. Missing mappings or two references for one Cast job stop migration.
   Do not infer a mapping from text or merge histories.
3. Let configure create the new private consistent schema-one backup, exclude
   concurrent ledger writes, and commit schema two with its legacy aliases.
   Preserve the reported backup path and recovery evidence. A retry creates a
   new backup and never overwrites an earlier one.
4. Verify schema-two integrity with the selected program and confirm deployment
   verification. Keep a schema-two-capable release selected after migration.

Migration preserves rows, exact write identity, aliases, text, corrections,
retractions, and frozen email history. Do not replay a legacy write with a changed
argument namespace. Direct install and `init` cannot perform this migration.

## Prepare the daily definition

1. Install Clew and initialize its selected ledger. Confirm that daily sending is
   separately authorized before enabling any binding.
2. Generate a new private definition file:

   ```sh
   clew-install schedule-definition --state-dir ABS_STATE --output ABS_NEW_FILE
   ```

   The output must be a new absolute file. Generation prepares private log paths
   and pins the selected release. It does not register, enable, or send.
3. Inspect the definition. Confirm `clew/daily-email`, local 09:00, no run-at-load,
   skipped overlap, the 180-second limit, and `halt-until-approved`.
4. Use coordinated deployment for schedule selection, or follow
   `chancery show clockwork.schedule.operate` for separately authorized
   registration and binding controls. Verify the selected digest and enabled
   intent. Preserve any existing failure halt.

A schedule grant authorizes every complete qualifying snapshot, including an
empty one. Provider size limits do not authorize truncation. Actual activation
depends on login and sleep. Resolve any uncertain submission through
`clew.digest.email` before approving continuation of a halted binding. Resume
permits future activation and does not retry an uncertain message.

## Back up or restore private state

1. Record the daily binding's enabled intent and failure state. Disable scheduled
   activation, stop Clew invocations, and wait for current commands to finish.
2. Preserve `ledger.sqlite3`, `email.sqlite3` when present, their SQLite sidecars,
   and `deployment-maintenance/` together in a private consistent backup. Keep the
   previous complete backup before restoring compatible history.
3. Reconcile uncertain write IDs and provider acceptance before replaying work.
   Older ledger history may no longer know a committed write. Older email history
   may permit a duplicate send.
4. Run `clew doctor` with a matching program and verify installation integrity.
   Restore only the previously enabled schedule intent after readiness. Preserve
   pre-existing failure halts and unresolved holds.

A rollback to schema one requires its matching ledger backup and reconciliation
of every post-migration report. Preserve delivery state; a schema-one ledger
backup does not authorize restoration of older email occurrences.

## Recover program selection

1. Preserve ledger, email, maintenance, and unresolved installation evidence.
   Disable the daily binding and settle sends before rollback to a release that
   does not understand daily email state.
2. Verify the retained owned release with
   `clew-install verify-release /absolute/owned/release`. Confirm its compatibility
   with the retained ledger before selection.
3. Select it with `clew-install recover --release /absolute/owned/release`.
   File recovery preserves private state. It does not downgrade the schema or
   retry mail. Follow retained coordinator recovery for an interrupted coordinated
   deployment so admission remains held until program and schedule selections
   are coherent.
4. Run `clew doctor` and `clew-install inspect`. Confirm selected file integrity,
   ledger compatibility, and the required Cast snapshot interface before restoring
   only captured enabled intent. Keep unresolved holds and incidents intact.

Stop when program and state versions do not match. Never repair a report by
editing SQLite. Use the application feature's append-only correction or retraction,
or retry an uncertain write with its original ID and identical arguments.

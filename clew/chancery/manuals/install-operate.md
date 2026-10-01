# Install Clew

Use this operation to install, inspect, migrate, prepare a schedule or
recover Clew. Read `chancery resolve clew.install.operate` for the required
`clew.state`, `clew.ledger.use`, `clew.application.track`, and
`clew.digest.email` contracts. They own the detailed behavior. This procedure
keeps prerequisites, effects, stop conditions, and verification in place.

General notes, named threads, plain external links, and generic reads require
no external reader. Explicit application features need Cast read contract two
for job search, first-job admission, and email context.
A nonempty schema-one migration also needs Platter read contract one. Email contract four supplies submission; Clockwork contract
four supplies activation and the shared service-health delay before a new halt. Clew runs no model requester.

Installation can select programs, initialize empty state, migrate legacy state,
and change explicitly supplied schedule intent. It adds no ledger notes or application reports
and performs no preparation or immediate send. Initialize and inspect commands
grant no note, mail, or status authority.

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
   binding, and drains sends before configuration. A schema-one or schema-two
   ledger uses the guarded migration below.
5. Confirm the retained deployment outcome and captured or explicit schedule intent.
   Deployment does not check artifact integrity, ledger integrity, or the Cast
   snapshot. Ordinary doctor and application checks remain separate.

Stop for missing dependencies, unresolved or foreign holds, an incomplete drain,
unsafe migration mappings, failed setup, or foreign selectors. Preserve operation evidence for coordinator recovery. Do not release
holds, approve incidents, or force an older incompatible program to obtain a
successful result.

## Install program files directly

1. Select a candidate and matching provider bundle. Confirm that
   any existing ledger is compatible. Direct installation does not migrate it.
2. Run the installer:

   ```sh
   clew-install install --binary /absolute/candidate/clew --bundle /absolute/clew/chancery
   ```

   Use `--home ABSOLUTE_PATH` for an explicit user home and
   `--expected-current absent|releases/HASH` when selection must match an exact
   prior condition. Stop if selectors belong to another owner .
3. Run `clew init` to create empty schema-three state or check compatible state.
   Stop for schema one or two, nonempty foreign state, or unsupported state.
4. Run `clew-install inspect` to read selected release metadata. Use `clew doctor`
   separately when a ledger diagnostic is needed; installation does not call it.
5. Run `clew --register-usage` to register command identities without reports.

The default ledger is `~/.local/share/clew/ledger.sqlite3`. Use global
`--state-dir ABSOLUTE_PATH` to select another private ledger for Clew commands.
State directories require mode 0700 and databases require mode 0600. Keep all
private state outside source and release trees.

## Migrate an older ledger

1. Use coordinated deployment to convert supported schema-one or schema-two state
   to schema three. Let admitted sends finish. Configure requires the sole
   run-owned email maintenance hold. Ordinary commands and `init` refuse older
   schemas; direct program installation does not migrate state.
2. Exclude concurrent ledger writers through the guarded migration transaction.
3. Resolve every retained legacy reference through Platter's public opportunity
   reader for schema-one state. Stop on missing mappings or two legacy references
   that select the same Cast job. An empty schema-one ledger needs no Platter read.
   Schema-two conversion is local and needs no external reader. Do not infer a
   mapping from company, role, or URL and do not merge histories.
4. Commit schema three, canonical external references, explicit application-report
   associations, legacy aliases, and exact write requests in one transaction.
   No original-schema copy is created. Failure before commit preserves prior data.
5. Read the retained deployment outcome. Preserve reported transaction evidence.
   Keep a schema-three-compatible program selected after migration. Restore only
   captured or explicit schedule intent after coherent activation.

Existing job identity becomes a `cast.job` external reference with an explicit
application-report association. Migration preserves entry IDs, sequence,
timestamps, supplied text, corrections, and retractions. Legacy aliases retain
their exact argument identity for retries. No old entry receives an invented
thread. Migration creates no new report and starts no collection, preparation,
completion check, or send.

Frozen email occurrences, message bytes, send keys, and acceptance receipts stay
unchanged. Old writers are rejected after commit. Do not replay a legacy write
with a changed argument namespace. Migration does not downgrade state.

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

## Recover program selection

1. Preserve ledger, email, maintenance, and unresolved installation evidence.
   Disable the daily binding and settle sends before rollback to a release that
   does not understand daily email state.
2. Select a retained owned release that supports the retained ledger schema.
   Program recovery does not check state compatibility.
3. Select it with `clew-install recover --release /absolute/owned/release`.
   File recovery preserves private state. It does not downgrade the schema or
   retry mail. Follow retained coordinator recovery for an interrupted coordinated
   deployment so admission remains held until program and schedule selections
   are coherent.
4. Run `clew-install inspect` to read release metadata. Ledger and Cast diagnostics
   remain separate. Restore only captured enabled intent. Keep unresolved holds and incidents intact.

Stop when program and state versions do not match. Never repair a report by
editing SQLite. Use the ledger feature's append-only correction or retraction,
or retry an uncertain write with its original ID and identical arguments.

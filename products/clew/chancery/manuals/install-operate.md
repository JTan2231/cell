# Install Clew

Use this operation to install, inspect, migrate, prepare a schedule or
recover Clew. Read `chancery resolve clew.install.operate` for the required
`clew.state`, `clew.ledger.use`, `clew.application.track`, and
`clew.digest.email` contracts. They own the detailed behavior. This procedure
keeps prerequisites, effects, stop conditions, and verification in place.

General notes, named threads, plain external links, and generic reads require
no external reader. Explicit application features need Milieu read contract two
for opportunity search, initial opportunity admission, and email context.
A nonempty schema-one migration also needs Platter read contract one. Email contract four supplies submission; Clockwork contract
four supplies activation and the shared service-health delay before a new halt. Clew runs no model requester.

Installation can select programs, initialize empty state, migrate legacy state,
and change explicitly supplied schedule intent. It adds no ledger notes or application reports
and performs no preparation or immediate send. Initialize and inspect commands
grant no note, mail, or status authority.

## Deploy through CI

1. Select the intended committed source and explicit product selection. Add any
   required dependency updates with another `--deploy PRODUCT`; an explicit
   deployment list does not add dependencies.
2. Set `daily_email_enabled` in a private settings file only when the intended
   schedule change is authorized.
   Omit it to preserve existing intent. Explicit true grants standing authority
   for the complete daily digest to Email's fixed personal recipient. Explicit
   false selects disabled intent.

   ```json
   {"clew":{"daily_email_enabled":false}}
   ```

3. Run `./ci.sh submit COMMIT --deploy clew` from the Cell root. Add
   `--settings /absolute/private/settings.json` when supplying settings.
   Telete integrates, validates, prepares signed programs, deploys, and emails
   the outcome. CI and Telete are the sole deployment route.
   The product command selects program and provider files,
   enters ordinary admission, initializes or migrates the ledger, and updates its
   daily definition. It preserves the saved schedule policy and enabled intent,
   unless `daily_email_enabled` overrides that intent. An absent binding remains
   absent when the setting is omitted. Deployment creates no maintenance hold,
   waits for owned scheduled activations before runtime publication, and preserves prior enabled intent.
4. Confirm the retained Telete job, deployment outcome, and captured or explicit
   schedule intent.
   Deployment does not check artifact integrity, ledger integrity, or the Milieu
   snapshot. Ordinary doctor and application checks remain separate.

Stop for unavailable required interfaces, an existing admission hold, unsafe
migration mappings, failed setup, or foreign selectors. Completed effects remain
after failure. Inspect them through the owning product interfaces before an
explicit new attempt. The executor performs no automatic retry or recovery.
Preserve holds and failure incidents.

## Inspect program publication

1. Submit program changes through CI. Stop if selectors belong to another owner.
2. Run `clew init` to create empty schema-four state or check compatible state.
   Stop for schema one through three, nonempty foreign state, or unsupported state.
3. Run `clew-install inspect` to read selected release metadata. Use `clew doctor`
   separately when a ledger diagnostic is needed; installation does not call it.
4. Run `clew --register-usage` to register command identities without reports.

The default ledger is `~/.local/share/clew/ledger.sqlite3`. Use global
`--state-dir ABSOLUTE_PATH` to select another private ledger for Clew commands.
State directories require mode 0700 and databases require mode 0600. Keep all
private state outside source and release trees.

## Migrate an older ledger

1. Use the product deployment command to convert supported schema-one, schema-two, or schema-three
   state to schema four under ordinary admission. Ordinary commands and `init`
   refuse older schemas; direct program installation does not migrate state.
2. Exclude concurrent ledger writers through the guarded migration transaction.
3. Resolve every retained legacy reference through Platter's public opportunity
   reader for schema-one state. Stop on missing mappings or two legacy references
   that select the same Milieu opportunity. An empty schema-one ledger needs no Platter read.
   Schema-two and schema-three conversion are local and need no external reader. Do not infer a
   mapping from company, role, or URL and do not merge histories.
4. Commit schema four, canonical external references, explicit application-report
   associations, legacy aliases, and exact write requests in one transaction.
   No original-schema copy is created. Failure before commit preserves prior data.
5. Read the retained deployment outcome. Preserve reported transaction evidence.
   Keep a schema-four-compatible program selected after migration. Preserve
   saved or explicitly requested schedule intent.

Existing opportunity identity becomes a `milieu.job` external reference with an explicit
application-report association. Migration preserves entry IDs, sequence,
timestamps, supplied text, corrections, and retractions. Legacy aliases retain
their exact argument identity for retries. No old entry receives an invented
thread. Migration creates no new report and starts no collection, preparation,
completion check, or send.

Frozen email occurrences, message bytes, send keys, and acceptance receipts stay
unchanged. Old writers are rejected after commit. Do not replay a legacy write
with a changed argument namespace. Migration does not downgrade state.

Schema-three conversion changes owned opportunity-reference names and stored retry fields
to Milieu. Use the renamed application argument with the original write ID for
an exact retry. This conversion preserves supplied content and opportunity identity.

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
   retry mail. Inspect the retained executor receipt after an interrupted
   deployment. Reconcile and acknowledge executor admission separately from any
   authorized product recovery; acknowledgement does not establish ledger or
   schedule correctness.
4. Run `clew-install inspect` to read release metadata. Ledger and Milieu diagnostics
   remain separate. Restore only captured enabled intent. Keep unresolved holds and incidents intact.

Stop when program and state versions do not match. Never repair a report by
editing SQLite. Use the ledger feature's append-only correction or retraction,
or retry an uncertain write with its original ID and identical arguments.

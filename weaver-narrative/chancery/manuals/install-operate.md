# Install and operate Weaver

Use this procedure to install, configure, inspect, back up, or recover Weaver.
Read `chancery resolve weaver.install.operate` for the required feature and
manager contracts. `weaver.lifecycle` owns configuration, state, readiness,
maintenance, and release guarantees. `weaver.narrative.write` owns authoring,
prompt selection, and exact assignment recovery.

Weaver reads an existing identity-bound Annals decisions library. It does not
provision that library. Read the installed `nucleus manual` before coordinated
maintenance. Keep settings, database, configuration, and backups private.
Installation and readiness checks create no narrative or model job. Manual
installation publishes local program bytes; it does not publish Git changes,
publish a narrative, send email, or authorize an authoring retry.

## Install or update

1. Select the authorized delivery route. For ordinary CI delivery, commit the
   intended change and submit `cell-ci submit COMMIT`, or `./ci.sh submit COMMIT`
   from the Cell root. The manager integrates, validates, attempts bounded
   repairs, deploys, and sends its deterministic outcome email. Verify its
   retained job outcome. For a separate authorized manual deployment, select
   a validated candidate on local `main`; the coordinator selects that commit.
2. Check the source prerequisites. Select an existing Annals decisions config,
   compatible Annals and Nucleus releases, and initialized Bazaar state with a
   complete `cell.prompts.weaver` selection. Import the reviewed migration seed
   before deploying these callers. Preserve selection version 1 and every
   referenced text version. Runtime reads and deployment do not supply missing
   prompt contents. The authoring feature owns the selection semantics.
3. Prepare the first installation's private settings file. `annals_config` is
   the only Weaver setting and must be an absolute path. Later deployments
   reuse its stored path unless settings select another path. The maintained
   installation selects the current user's `~/.local/bin/annals`.

   ```json
   {"weaver":{"annals_config":"/absolute/Annals/decisions/config.toml"}}
   ```

4. Preview the selected products and dependencies from the Cell root. Stop if
   the plan requires a choice or effect outside the authorized endpoint.

   ```sh
   ./deploy.sh plan weaver
   ```

5. Invoke the coordinator when deployment is authorized. Use the settings file
   for first installation or an intended reader change; later deployments can
   reuse existing configuration.

   ```sh
   ./deploy.sh weaver --settings /absolute/weaver-settings.json
   ```

   The coordinator holds and drains Weaver, selects the program and provider,
   and calls the product initializer for configuration. It releases only its own
   hold. It performs no artifact-integrity, state-integrity, Annals-readiness, or
   Nucleus-readiness checks. Product initialization retains its ordinary rules.
   Do not invoke direct installer `install` or `recover`; those routes are refused.

6. Read selected release metadata with `weaver-install inspect`. Use `weaver doctor`
   separately when live state or dependency diagnostics are needed. Doctor is not
   an installation gate. Read the installed Chancery pages for their contracts.

7. Register the installed command inventory with `weaver --register-usage`.
   Confirm that the coordinator released its own holds. Preserve other owners'
   holds. Treat a readiness or documentation gap as its reported outcome;
   catalog presence alone does not establish runtime success.

## Inspect or reconfigure

1. Inspect the configured reader and current evidence. The incomplete status
   snapshot declares `weaver/author` as on demand; use doctor and maintenance
   for live readiness and settlement.

   ```sh
   weaver config
   weaver doctor
   weaver status-snapshot --json
   weaver maintenance status
   ```

2. Select an existing identity-bound Annals decisions config when changing
   reading configuration. Initialize or reconfigure through the supported
   command. The operation takes the runner lock, preserves existing documents,
   and refuses unsupported state.

   ```sh
   weaver init --annals-config /absolute/decisions/config.toml
   weaver init --annals-config /absolute/decisions/config.toml --annals-binary /absolute/annals
   ```

3. Read `weaver config` and run `weaver doctor` again. Verify the intended reader
   and source readiness without authoring a test narrative. Stop on unknown or
   incompatible evidence; do not edit SQLite or initialize replacement state.

## Back up or recover

1. Acquire an explicit maintenance owner for backup or attended maintenance.
   Preserve any pre-existing holds.

   ```sh
   weaver maintenance hold RUN_ID
   weaver maintenance status
   weaver maintenance drain
   ```

2. Settle admitted work before copying state. Resume each exact document/job ID
   under `weaver.narrative.write` when needed. Cancel only an exact Nucleus job
   that the authorized operation intends to abandon. Drain must prove that no
   Weaver process or nonterminal Weaver Nucleus job remains. Unavailable
   inventory stays unknown and stops the procedure.

   ```sh
   weaver resume DOCUMENT_ID
   nucleus jobs cancel DOCUMENT_ID
   ```

   After an intended cancellation, resume the same ID to collect its outcome.
   Resume reuses the saved request and job; it does not create another attempt.
   Inspect `weaver show DOCUMENT_ID` if runtime failure follows saved Markdown.
3. Save a consistent SQLite backup of `weaver.sqlite` and the private
   `config.json`, or copy the database while Weaver is drained. Both are under
   `~/Library/Application Support/Weaver`. Back up Nucleus records and
   credentials separately under its own rules. Schema 1 has no predecessor
   migration and does not import retired Weaver workflow records.
4. Recover an interrupted deployment through the coordinator's retained
   transaction. Follow its original ownership and exact candidate. The next
   ordinary deployment command uses an unresolved transaction for recovery;
   there is no separate public deployment resume route. Before publication the
   prior installation stays selected; after publication Weaver can finish
   forward with recorded reading configuration and supported state. Keep the
   named hold if recovery cannot be proved. Do not delete holds or change
   database rows to bypass the failure.
5. Confirm complete drain before releasing the hold acquired for attended
   maintenance. Source and readiness diagnostics remain separate operations.

   ```sh
   weaver maintenance release RUN_ID
   ```

   Release only that owner. Uninstall retains private state; clearing it needs
   a separate decision. No backup command, general restore automation, recovery
   latency, retention horizon, release cadence, or future model availability is
   promised by this procedure.

CLI usage recording needs a nonempty `CODEX_THREAD_ID`. It records command
identity, time, and thread ID without arguments, output, or outcomes. Recording
errors preserve command results. No procedure here authorizes narrative
publication, source mutation, credential changes, or state deletion.

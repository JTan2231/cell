# Register and recover command usage

Use this operation to register an installed program's full command inventory,
inspect recorded activity, or create and restore a compatible private journal
backup. Read `chancery resolve chancery.usage.operate` for the complete journal
feature, including writer APIs, attribution, query semantics, privacy, and
failure limits. There is no daemon or network transport.

The journal observes command-handler entry. It does not prove completion,
domain success, exhaustive usage, or model consumption. This procedure does
not authorize product work, retrying an uncertain append, pruning history,
changing stored identities, or migrating an unsupported journal.

## Select and check the journal

1. Select the intended current-user journal. The default is
   `~/Library/Application Support/Chancery/usage.sqlite3`. An absolute
   `CHANCERY_USAGE_DB` overrides it; an empty value uses the default.
2. Confirm whether the journal is absent, supported Chancery schema one, or
   unknown state. Preserve any existing file and sidecars. Do not create a
   replacement for populated, foreign, or unsupported state.
3. Identify the programs whose installation or update requires registration.
   Copying binaries or publishing catalog entries does not register commands.
   Rebuild participating binaries when compiled recorder rules change. Update
   their owning wrappers, hooks, and pinned Clockwork brokers through their
   installation procedures.

## Register the installed inventory

1. Run each selected program's exact registration mode after installation:

   ```sh
   chancery --register-usage
   annals --register-usage
   annals-usage --register-usage
   ```

   Use only the participating programs being installed or updated. Annals has
   two programs; register both when updating that release. Each mode initializes
   only an empty journal through Chancery's owning API and adds the program's
   full declared inventory. It runs no product work and does not observe itself.
2. Use the explicit Chancery administrative interfaces when registering an
   inventory manually:

   ```sh
   chancery usage init
   chancery usage register SYSTEM COMMAND_ID...
   ```

   `init` initializes supported empty state and registers Chancery's commands.
   `register` requires an initialized journal. Registration can commit earlier
   identities before a later error. Repeat the same registration after resolving
   its reported cause; unchanged identities are idempotent. Historical commands
   removed from a newer release remain registered.
3. Verify the selected identities:

   ```sh
   chancery usage systems
   chancery usage commands --system SYSTEM
   ```

4. Confirm that expected commands appear, including unused commands with zero
   counts and null latest observation times. Registration confirms identity
   retention; it does not prove current instrumentation or installation.

Stop on a foreign or unsupported journal schema, invalid identity, unavailable
private file, or unexplained registration result. Product programs do not
migrate the journal. Preserve history and diagnose through Chancery's owning
interface rather than editing SQLite or disabling its triggers.

## Inspect recorded activity

1. Select the requested system, thread, and time range. Use whole Unix seconds;
   `--since` is inclusive and `--until` exclusive. Do not combine `--thread`
   with `--unattributed`.
2. Read counts or a bounded insertion-ordered page:

   ```sh
   chancery usage commands --system annals --since 1700000000
   chancery usage events --thread THREAD_ID --after 0 --limit 100
   chancery usage events --unattributed
   chancery --json usage commands
   ```

3. Preserve `next_cursor` only for this database history and use `has_more` to
   continue paging. Event limits are 1–1000, default 100. Reads preserve journal
   rows, but CLI dispatch can append its own metadata-only observation.
4. Report the selected recorded invocations. A zero count means no selected
   observations. Missing, disabled, failed, and uninstrumented observations
   remain uncovered; do not infer that an absent command was never used.

A failed append must preserve the product result. No automatic recording
retry or duplicate suppression exists; an uncertain append may have committed.
Inspect available evidence and accept the observation gap. Do not retry product
work to obtain a journal row.

## Back up and restore

1. Create a consistent SQLite backup separately from product data. Protect it
   as private current-user metadata. For a quiescent file backup, stop every
   writer and keep the journal's sidecars with the file.
2. Identify the exact compatible backup and schema before restoration. Preserve
   the old history and current program selection. Stop every journal writer
   before replacing selected files; Chancery has no central daemon to stop them.
3. Restore the compatible journal/schema pair under private file permissions.
   The regular journal file must be 0600. Do not open a newer unsupported
   journal with an older program or recreate a populated journal.
4. Read systems, command counts, and selected events to verify the restored
   history. Re-establish cursors from that history. Repeat unchanged registration
   for installed inventories when needed; registration preserves prior records.
5. Resume the writers after verification. Program rollback leaves newer rows
   intact and does not itself restore journal history.

Stop if writer quiescence, backup consistency, schema compatibility, private
ownership, or retained-history verification cannot be established. There is no
retention cleanup, migration, deletion, or trigger-bypass procedure. Keep
unknown state available for diagnosis.

## Privacy and completion

Registration and usage store opaque system, command, and thread identities,
observation IDs, and whole Unix-second timestamps. They contain no prompts,
credentials, arguments, output, duration, or outcome. Filesystem permissions
are the current-user trust boundary; supplied thread IDs are cooperative
attribution rather than authenticated Codex history.

Completion requires the intended registered identities or verified compatible
restored history. Readiness and success of any product command remain owned by
that product. Journal registration is a separate installation result.

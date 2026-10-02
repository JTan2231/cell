# Install or diagnose Clockwork

Installation places resources and runs the declared product setup commands.
It does not gate completion on persistent-state validation, artifact
integrity audits, or runtime readiness checks. The product's ordinary diagnostics
and runtime checks remain available separately.

Use this procedure for authorized installation, diagnosis, program
rollback, selector detach, or explicit state migration. Read the required
`clockwork.installation` and `clockwork.schedule.operate` contracts for release,
state, and schedule details. Direct program installation changes owned program
and provider selectors; it does not operate product definitions or bindings.
Manifest deployment also refreshes existing enabled broker bindings.

Public `clockwork` commands print plain text by default. Add `--json` when
a script or typed integration must parse a result. The flagged response
schemas and the private broker output are unchanged.

## Prepare and install

1. Select the candidate binary, installer, and provider bundle at absolute paths.
2. Run the installer with `install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE`.
   Use `--home ABSOLUTE_HOME` for an alternate home and `--expected-current` to
   retain a captured selector expectation.
3. Register command inventory with `clockwork --register-usage`.

The installer prepares resource directories and publishes command and provider
selectors. It does not audit release bytes, compare provider versions, validate
persistent state, or run doctor. It preserves owned-path boundaries, writer
locks, and atomic selector replacements. Failed instructions retain completed
changes. Runtime diagnosis remains available separately
through `clockwork doctor`; that command's state effects are unchanged.

## Coordinate broker refresh

1. Authorize Cell deployment separately, then use `./deploy.sh clockwork`.
   Select any product updates explicitly; the executor adds no products.
2. Let the product command select Clockwork files. If its runtime database exists,
   it lists current bindings and switches each enabled binding to the same
   definition digest through the selected broker. Disabled bindings stay disabled.
   It creates no maintenance hold and temporarily disables no binding.
3. Inspect the retained result after interruption. Completed selector and binding
   changes remain. Reconcile and acknowledge executor admission separately from
   any authorized Clockwork recovery. No automatic retry or recovery follows.

Stop on a failed file or binding operation. Refresh preserves definitions and
failure incidents; it approves no halt and retries no product work. A run-at-load
definition can start product work when its enabled binding is refreshed.
Direct installation and program rollback alone do not refresh pinned broker paths.

## Roll back a program release

1. Preserve failure evidence. Resolve `install/previous` to the canonical owned
   retained release and select its installer.
2. Recover through the explicit candidate reader:

   ```sh
   <TRUSTED_CLOCKWORK_INSTALL> recover \
     --release <VERIFIED_PREVIOUS_RELEASE_DIRECTORY> \
     --chancery /absolute/path/to/chancery
   ```

3. Verify program, installer, provider, and current selector together. Inspect
   product bindings separately; program rollback does not rewrite their plists.

Do not prune a release referred to
by a generated plist or running activation. Do not restore an older broker
that ignores active notification eligibility or delegated claims, or run an
older broker while `failure-checks.json` exists. Preserve the incident database,
`failure-checks.json`, notification check/routing sidecars, and EMT correspondence
together. SQLite schema two remains unchanged; that alone does not prove an
older broker compatible with the new sidecar.

## Detach owned selectors

1. Disable every binding through `clockwork.schedule.operate`. Verify quiescence
   and absence of every regular or symbolic `org.clockwork.*.plist`. Plist
   absence alone does not prove that no manual child remains.
2. Invoke a trusted tested installer:

   ```sh
   <TRUSTED_CLOCKWORK_INSTALL> uninstall
   ```

3. Verify owned command, installer, provider, current, and previous selectors
   are detached. Retained releases, database, history, locks, and product logs
   remain. An absent private state root may be created for the shared
   installation lock, without creating a runtime database.

Stop on remaining plists, unresolved transitions, unknown process ownership,
or foreign selectors. Detach neither boots out schedules nor kills children.
Deleting retained state or releases is a separate destructive operation with
exact targets and proof that nothing refers to them.

## Migrate database schema

1. Hold all product schedules and quiesce Clockwork commands. Finish or recover
   running rows and pending binding transitions with the old binary. Capture
   prior bindings, definitions, enabled intent, product pauses, and releases.
2. Run the tested new binary with `migrate`. It refuses running rows and pending
   transitions, then changes the schema transactionally in place. Verify the
   returned schema version before proceeding.
3. Register schema-two product definitions under product maintenance. Preserve
   disabled selection and transfer failure-owned pauses with
   `binding halt KEY --code CODE --occurrence ID` before removing an old gate.
   Keep item recovery, user pauses, and maintenance. Do not call resume.
4. Verify `failure_policy_active: true` for each upgraded selection. Refresh
   enabled plists to the compatible exact broker before releasing maintenance.
   A migrated schema-one definition retains its original digest and policy.
5. Recover program selection with a release compatible with the retained schema.
   Keep failed state and newer incident evidence. There is no reverse schema
   operation.

Stop if state compatibility, disabled intent, or newer
halt preservation cannot be proved. Program deployment never performs migration.
Chancery compatibility and installed version evidence do not establish live
Email, Iatreion, launchd, or product readiness.

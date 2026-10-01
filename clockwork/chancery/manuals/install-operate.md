# Install or diagnose Clockwork

Installation places resources and performs setup under the documented maintenance
boundary. It does not gate completion on persistent-state validation, artifact
integrity audits, or runtime readiness checks. The product's ordinary diagnostics
and runtime checks remain available separately.

Use this procedure for authorized installation, diagnosis, program
rollback, selector detach, or explicit state migration. Read the required
`clockwork.installation` and `clockwork.schedule.operate` contracts for release,
state, and schedule details. Direct program installation changes owned program
and provider selectors; it does not operate product definitions or bindings.
Coordinated deployment also suspends and refreshes captured broker bindings.

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
locks, and selector compensation. Runtime diagnosis remains available separately
through `clockwork doctor`; that command's state effects are unchanged.

## Coordinate broker refresh

1. Authorize coordinated Cell deployment separately, then use
   `./deploy.sh clockwork`. It captures the complete binding inventory before
   maintenance and disables each binding while retaining selection and halts.
2. Let product adapters prepare their definitions under their own holds. After
   holds are released, verify that each captured enabled binding is restored
   through the final broker and that its generated plist pins that release.
   Previously disabled bindings stay disabled. This phase precedes EMT activation.
3. Recover an interruption from the retained original inventory. Re-establish
   suspension before configuration repair. Preserve captured intent and open
   incidents; do not infer intent from temporary deployment disablement.

Stop when unknown binding or projection changes prevent coherent recovery.
No stage approves a failure halt or retries product work. Direct installation
and program rollback alone do not refresh pinned broker paths.

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

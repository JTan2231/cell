# Install or diagnose Clockwork

Use this procedure for authorized installation, diagnosis, verified program
rollback, selector detach, or explicit state migration. Read the required
`clockwork.installation` and `clockwork.schedule.operate` contracts for release,
state, and schedule details. Direct program installation changes owned program
and provider selectors; it does not operate product definitions or bindings.
Coordinated deployment also suspends and refreshes captured broker bindings.

## Prepare and install

1. Build and validate one tested candidate binary, Rust installer, and complete
   provider bundle. Use absolute paths to regular executable candidates and
   an explicitly supplied Chancery reader that supports provider schema four.
   Confirm binary, installer, and provider release versions match exactly.
   Deployment requires its own authority; ordinary CI delivery uses the manager.
2. Inspect prior selectors, retained release integrity, and foreign-path risks.
   Run the tested installer:

   ```sh
   <TESTED_CLOCKWORK_INSTALL> install \
     --binary <TESTED_CLOCKWORK_BINARY> \
     --bundle /absolute/clockwork/chancery \
     --chancery /absolute/path/to/chancery
   ```

   Use `--home ABSOLUTE_HOME` only for intentional alternate-home or isolated
   installation. Use `--expected-current absent|releases/HASH` when the caller
   must enforce its captured selector expectation.
3. Verify staged content identity and the retained previous generation. The
   candidate reader must validate the exact staged bundle before any selector
   mutation and discover every indexed entry through the selected provider
   before commit. The whole overview, feature, and procedure tree belongs to
   the immutable release.
4. Verify installed selection and published reading:

   ```sh
   clockwork --version
   clockwork --help
   chancery product clockwork
   chancery list --provider clockwork
   chancery show clockwork.install.operate
   chancery resolve clockwork.install.operate
   chancery doctor
   clockwork --register-usage
   ```

5. Run `clockwork doctor` only when its bounded effects are intended. It may
   prepare private directories, initialize an empty unversioned schema-two
   store, and mark running rows lost after broker and child absence is proved.
   Inspect its quick-check, executable, launchctl, and transition evidence.
   It executes no product and proves no future timer or domain success.

Stop if version, candidate-reader availability, provider schema, exact content,
selector ownership, or coherent rollback cannot be proved. Do not replace a
foreign path or bypass a tamper check. Identical installation is idempotent.
Failure before commit restores prior selectors, or detaches all owned public
selectors when coherent restoration cannot be completed. Retain reported paths
and evidence before retrying.

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
   retained release and verify it with a trusted tested installer.
2. Recover through the explicit candidate reader:

   ```sh
   <TRUSTED_CLOCKWORK_INSTALL> recover \
     --release <VERIFIED_PREVIOUS_RELEASE_DIRECTORY> \
     --chancery /absolute/path/to/chancery
   ```

3. Verify program, installer, provider, and current selector together. Inspect
   product bindings separately; program rollback does not rewrite their plists.

Do not execute an unverified retained installer or prune a release referred to
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

## Migrate or recover database schema

1. Hold all product schedules and quiesce Clockwork commands. Finish or recover
   running rows and pending binding transitions with the old binary. Capture
   prior bindings, definitions, enabled intent, product pauses, and releases.
2. Run the tested new binary with
   `migrate --backup /absolute/new-backup-directory`. It refuses running rows,
   checkpoints SQLite, retains private database and sidecars, and applies only
   the schema change. Verify its receipt and backup before proceeding.
3. Register schema-two product definitions under product maintenance. Preserve
   disabled selection and transfer failure-owned pauses with
   `binding halt KEY --code CODE --occurrence ID` before removing an old gate.
   Keep item recovery, user pauses, and maintenance. Do not call resume.
4. Verify `failure_policy_active: true` for each upgraded selection. Refresh
   enabled plists to the compatible exact broker before releasing maintenance.
   A migrated schema-one definition retains its original digest and policy.
5. For rollback across schema two, quiesce again and restore a matching
   schema-one database and sidecars with compatible Clockwork/product releases,
   prior definitions and plists. Retain the failed store and newer incident
   evidence. A pre-halt backup cannot erase a later halt or authorize work.

Stop if backup coherence, old-state compatibility, disabled intent, or newer
halt preservation cannot be proved. Program deployment never performs migration.
Chancery compatibility and installed version evidence do not establish live
Email, Iatreion, launchd, or product readiness.

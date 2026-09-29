# Install, configure, and recover Cast

Use this operation for an intended program installation, local configuration,
readiness diagnosis, source-control change, or employer-ownership repair.
Cast owns those results. It does not authorize provider purchases, a new
schedule, credential replacement, destructive state reset, or downstream
application work.

Read `chancery resolve cast.install.operate` for this procedure and its required
contracts: `cast.installation` owns program lifecycle and wrapper credentials;
`cast.state` owns state, configuration, source controls, and repair;
`cast.discovery.collect` owns collection behavior; and
`cast.discovery.explore` owns the evidence used for verification. This manual
keeps the prerequisites, effects, stop conditions, and steps needed to operate.

## Select the operation and prerequisites

1. Identify the intended outcome and selected operator home or state directory.
2. Inspect existing state and configuration before changing policy. Keep the
   current complete configuration and consumed budgets.
3. Select a trusted tested installer and validated matching binary/bundle for
   program work. Use user-owned paths and the supported macOS shell tools.
4. Stop callers before state recovery or ownership reconciliation. Obtain a
   private consistent SQLite backup, including live sidecars when relevant,
   before state recovery.
5. Keep keys outside arguments, configuration, database rows, and logs.

Stop if the intended selector belongs to another owner, candidate and provider
versions differ, the retained program is unverified, or an active writer would
be displaced. Stop before destructive or incompatible state work without a
defined compatible backup and recovery choice. Program recovery does not
restore discovery state or configuration.

## Install the validated candidate

Use `cell-ci submit COMMIT` for ordinary CI delivery. The manager integrates,
validates, attempts bounded repairs, deploys, and emails the outcome. For an
explicitly authorized manual installation or recovery:

1. Run the tested candidate installer with absolute candidate paths:

   ```sh
   <TESTED_CAST_INSTALL> install --binary <TESTED_CAST_BINARY> \
     --bundle /absolute/path/to/cast/chancery
   ```

2. Supply `--home PATH` if selecting another operator home. Supply
   `--expected-current absent|releases/HASH` when the exact prior selection is
   required.
3. Verify the selected command versions and help output:

   ```sh
   cast --version
   cast --help
   cast-install --help
   ```

4. Inspect the matching publication with `chancery product cast` and
   `chancery show cast.installation` when Chancery is available.
5. Run `cast --register-usage` after installation or update. This registers
   command inventory without product work.

Installation selects a content-addressed payload, frontend, installer, and
exact provider bundle through one atomic `current` release. It creates no
database or schedule and sends no provider request. A failed switch restores
prior Cast selectors. Catalog presence and version checks do not prove remote
authentication or current provider allowance.

## Initialize, diagnose, or replace configuration

1. Select state with `--state-dir PATH` or `CAST_STATE_DIR` when the default
   `~/.local/share/cast` is not intended.
2. Initialize missing state and inspect local readiness:

   ```sh
   cast init
   cast doctor
   cast config show
   cast status --json
   ```

3. Inspect the complete configuration before changing queries, intervals,
   adapter settings, budgets, or `automatic_excluded_ats`.
4. Write the intended complete JSON to a private file, then apply and verify it:

   ```sh
   cast config set --file /absolute/path/to/config.json
   cast config show
   cast doctor
   cast status --json
   ```

Initialization preserves existing records and consumed budgets and collects
nothing. Configuration replacement affects later collection and does not reset
usage, purchase credits, or change provider billing. The ATS exclusion defaults
to Ashby when omitted; an explicit empty array permits all supported ATS
providers subject to source enrollment. Older programs can reject configuration
written with this field.

The installed frontend executes user-owned `.zshrc` with trace/output
suppressed and passes the provider keys through its restricted environment.
Its shell commands and side effects remain user-owned. `doctor` can check
local configuration and credential presence. Actual provider authentication,
balances, and collection require provider interactions.

If collection failed or was partial, inspect the last run, source health,
coverage, and budget diagnostics. Preserve successful observations and local
charges. Do not erase state to clear allowance or treat an absent error as
complete coverage. Running collection is a separate invocation under
`cast.discovery.collect`.

The Cell coordinator uses `cast init` after program selection. Its optional
`state_dir` and `config_file` settings are absolute paths. A supplied file
replaces complete configuration; omitted settings preserve current values.
This setup creates no collection schedule.

## Add or disable a source

1. Choose an ordinary website or supported ATS source and inspect the intended
   company identity.
2. Add an ordinary website using an existing company when appropriate, or add
   a supported ATS board without a company override:

   ```sh
   cast source add https://employer.example/careers --company-id COMPANY_ID
   cast source add https://job-boards.greenhouse.io/EMPLOYER
   cast sources list
   ```

3. Disable ordinary collection for an exact source when intended:

   ```sh
   cast source disable SOURCE_ID
   cast sources list
   ```

An ordinary website without `--company-id` creates or reuses a hostname
candidate. Supported ATS URLs use canonical provider/tenant owners and reject
a company override. Disabling retains the source and its jobs. It does not
block explicit `job collect` requests. Inspect the resulting source ownership
and enabled setting; adding a source does not prove successful retrieval.

## Reconcile older employer ownership

1. Stop collection for the selected state directory.
2. Run the supported local repair and inspect its result:

   ```sh
   cast state reconcile-ownership
   cast export --json
   cast status --json
   ```

3. Verify the reported `moved_sources`, `moved_jobs`, `quarantined_jobs`,
   `renamed_candidates`, and `cleared_shared_identities` counts and the affected
   source/job associations.
4. Confirm that source/job IDs, paid request usage, run history, query coverage,
   and cursors remain retained. Resume only the callers stopped for this work.

The repair holds one mutation lock and commits one transaction. It corrects
ATS ownership, sets older or affected shared-host JSON-LD jobs to `unknown`,
marks sources for collection, and corrects affected candidate identities.
Changed jobs and companies gain revisions. Repetition leaves material records
unchanged but advances the snapshot revision. The repair sends no provider
request and does not establish current posting availability.

## Recover programs or prepare state recovery

1. Resolve `install/previous` to its canonical owned retained release directory.
2. Select it with a trusted tested installer:

   ```sh
   cast-install recover --release ABSOLUTE_RELEASE_DIRECTORY
   ```

3. Verify command identity, help output, and matching documentation. Register
   usage after the selection.
4. Run `cast doctor`, `cast config show`, and `cast status --json` against the
   intended state to verify compatibility before collection.

The installer verifies a retained legacy or `cell-install-v2` release before
selection. Do not execute an unverified retained installer or edit a
content-addressed bundle. A failed switch restores the prior selectors.

An abruptly killed deployer can leave `.update-lock`. Confirm that no Cast
deployer is running before removing a stale installation lock and rerunning
the tested candidate. Never remove another active writer's lock. Runtime
collection uses a separate kernel-backed lock.

For state recovery, stop all selected-state callers and preserve a private
consistent SQLite backup, including live sidecars when relevant. Program
recovery leaves discovery state unchanged. This release provides no automatic
database migration, pruning, destructive reset, or state uninstaller. Stop
when a proposed recovery requires unsupported row edits or an incompatible
program/state pair.

Keep state, exports, and diagnostics private. Cast starts no Nucleus, CRM,
Email, or computer-use work. Chancery reads documentation only; neither its
catalog nor successful local checks grants authority for new external work.

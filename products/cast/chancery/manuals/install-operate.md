# Install, initialize, and recover Cast

Use this operation for program installation, state initialization, local
readiness diagnosis, or source enrollment. Cast owns those results. It does
not authorize a new schedule, destructive state reset, or downstream
application work.

Read `chancery resolve cast.install.operate` for this procedure and its required
contracts. `cast.installation` owns program lifecycle and frontend environment.
`cast.state` owns state, local status, readiness, and source enrollment.
`cast.discovery.explore` owns the record and export meanings used for
verification.

## Select the operation and prerequisites

1. Identify the intended outcome and selected operator home or state directory.
2. Inspect existing state before changes. Preserve its accepted records.
3. Select a trusted tested installer and supplied binary/bundle for program
   work. Use user-owned paths and the supported macOS shell tools.
4. Stop callers before state maintenance or recovery.

Stop if the intended selector belongs to another owner or an active writer
would be displaced. Stop before unsupported destructive or incompatible state
work. Program recovery does not restore Cast data.

## Install the candidate

Use `cell-ci submit COMMIT` for ordinary CI delivery. The manager integrates,
validates, attempts bounded repairs, deploys, and emails the outcome. For an
explicitly authorized manual installation or recovery:

1. Run the tested candidate installer with absolute candidate paths:

   ```sh
   <TESTED_CAST_INSTALL> install --binary <TESTED_CAST_BINARY> \
     --bundle /absolute/path/to/cast/chancery
   ```

2. Supply `--home PATH` to select another operator home. Supply
   `--expected-current absent|releases/ID` when the exact prior selection is
   required.
3. Read `cast-install inspect` for selected release metadata. Run ordinary
   diagnostic commands separately when requested.
4. Inspect the matching publication with `chancery product cast` and
   `chancery show cast.installation` when Chancery is available.
5. Run `cast --register-usage` after installation or update.

Installation selects a retained payload, frontend, installer, and exact
provider bundle through one atomic `current` release. Direct installation
creates no database or schedule and sends no provider request. A failed
switch retains completed selector changes for explicit recovery. Catalog
presence and release metadata do not prove usable local state.

## Initialize and diagnose state

1. Select state with `--state-dir PATH` or `CAST_STATE_DIR` when the default
   `~/.local/share/cast` is not intended.
2. Initialize missing state and inspect local readiness:

   ```sh
   cast init
   cast doctor
   cast status --json
   ```

3. Verify supported database schema 2 and the intended record counts.
4. Inspect retained records with `cast jobs list` or `cast export --json` when
   the requested result requires record verification.

Initialization creates schema-two current state when the database is missing.
It preserves supported existing state and collects nothing. Schema-one state
is rejected without migration. Stop if the selected directory requires an
unsupported migration or manual row repair.

Doctor checks usable local state. It reads no provider keys or collector
configuration. The installed frontend does not source shell configuration.
Local readiness does not establish current source availability.

The Cast deployment recipe uses native state APIs and the product lock after
program selection. Its optional `state_dir` setting is an absolute path.
Setup initializes missing schema-two state and rejects unsupported schemas.
It does not invoke the selected CLI and creates no schedule. There is no
`config_file` setting.

Ordinary commands print readable text by default. Add global `--json` when a
caller parses output. Ordinary status uses schema 3. List/search pages remain
schema 2; export artifacts remain schema 1. The separate operational
`status-snapshot --json` protocol remains schema 1. Read `cast.state` and
`cast.discovery.explore` for full output and error rules.

## Add a source

1. Choose an ordinary website or supported ATS source and inspect the intended
   company identity.
2. Add an ordinary website using an existing company when appropriate, or add
   a supported ATS board without a company override:

   ```sh
   cast source add https://employer.example/careers --company-id COMPANY_ID
   cast source add https://job-boards.greenhouse.io/EMPLOYER
   cast sources list
   ```

3. Verify the resulting source identity, URL, and company association.

An ordinary website without `--company-id` creates or reuses a hostname
candidate. Supported ATS URLs use canonical provider/tenant associations and
reject a company override. This compatibility association does not establish
the optional source operator or any job employer. Enrollment performs no
retrieval and does not establish collection success.

## Recover programs or prepare state recovery

1. Resolve `install/previous` to its canonical owned retained release directory.
2. Select it with a trusted tested installer:

   ```sh
   cast-install recover --release ABSOLUTE_RELEASE_DIRECTORY
   ```

3. Read selected release metadata and matching documentation. Register usage
   after selection.
4. Confirm that the selected program accepts database schema 2 and does not
   require removed collector configuration. Keep requested diagnosis separate
   from program recovery.

The installer reads supported retained installation metadata before selection.
Use a trusted installer. Keep retained release files unchanged. A failed
switch retains completed selector changes.

An abruptly killed deployer can leave `.update-lock`. Confirm that no Cast
deployer is running before removing a stale installation lock and rerunning
the tested candidate. Never remove another active writer's lock. Runtime
mutation uses a separate kernel-backed lock.

Program recovery leaves Cast data unchanged. Schema-one-only programs cannot
read schema-two state; this release cannot read schema-one state. There is no
automatic database migration, pruning, destructive reset, or state uninstaller.
Older releases can read schema-two exports but fail status or diagnosis when
they require collector configuration absent from new state. Recovery does not
recreate configuration. Stop when recovery requires unsupported row edits or an incompatible
program/state pair.

Keep state, exports, and diagnostics private. Cast starts no Nucleus, CRM,
Email, or computer-use work. Chancery reads documentation only; its catalog
and local checks grant no authority for new external work.

## Deployment recipe

`cast-install deploy` reads one schema-two Cell recipe request from stdin.
The product command selects supplied programs and initializes missing state
through the product lock. The manifest executor runs the instruction and
records its exit status. It does not inspect application output or create a
maintenance hold, drain work, or recover prior effects. A failed instruction
leaves completed changes in place. Use explicit product recovery when required.

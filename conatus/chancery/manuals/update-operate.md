# Initialize and operate Conatus

Use this procedure for initialization, update, instruction selection, recovery,
installation, schedules and maintenance. Read `conatus.processing` for complete
intake and interpretation behavior, `conatus.service` for release and scheduler
behavior, `conatus.library.explore` for observation meaning, and
`conatus.digest.email` for disclosure and uncertain-send recovery.
`chancery resolve conatus.update.operate` includes the required contracts.

Conatus owns local intake, cursor, outgoing documents, handoff receipts, gates
and selected instructions. Annals owns retained sources, graph, evidence,
interpretation and domain recovery. Clockwork owns activation and incidents.
Do not edit another product's database or spool.

## Select state and prerequisites

Use global `--state-dir ABS_STATE` for private Conatus state. The default is
`CONATUS_STATE_DIR` or `~/Library/Application Support/Conatus`. Product commands
return JSON success data or an error; `--json` is optional. Keep private source
text, full documents, evidence and model context out of routine logs.

1. Select the exact supported Annals executable, writable general-library state,
   explicit existing decisions-library config and named general library.
2. Verify initialized private Bazaar state and a complete `cell.prompts.conatus`
   selection. An absolute `CELL_BAZAAR_DATABASE` can select another database.
   Runtime reads and deployment do not import missing text.
3. Verify the pinned library identities before update or recovery. For model
   work, establish Annals' configured authenticated Nucleus path.
4. Stop on absent selection, mismatched identity, unreadable state or uncertain
   domain outcome. Do not redirect libraries or reset the baseline to recover.

Installation, initialization and activation require separate authority. Update,
retry and re-examination can use account allowance and apply valid material
changes through Annals. These procedures do not authorize publication,
credential changes, historical import, user-data cleanup or unrelated lifecycle
actions.

## Initialize or rebind Annals

1. Run initialization with the intended state and explicit selections:

   ```sh
   conatus --state-dir ABS_STATE init --annals ABS_ANNALS --decisions-config ABS_CONFIG --library conatus
   ```

   Add `--annals-state-dir ABS_PATH` when a different Annals catalog is selected.
   First initialization creates or selects the library, selects exact instructions,
   pins both library identities and starts at the current accepted-feed watermark.
   It starts no model, imports no earlier decisions and enables no schedule.
2. Run `conatus --state-dir ABS_STATE --json config` and
   `conatus --state-dir ABS_STATE status`. Verify both library selections and
   the saved baseline before processing.
3. Repeat `init` only with the same library, decisions config and Annals state
   root to rebind the Annals executable. Verify `initialized:false` and the
   `rebound` result. Confirm preserved cursor, records, instructions and pause.
4. Update any selected immutable schedule separately after a Conatus program
   change. Direct installation and executable rebinding do not retarget it.

## Update and inspect the result

1. Read `conatus status` to establish pause, intake and handoff state.
2. Run `conatus update`. It consumes new accepted documents, queues frozen
   sources and dispatches the dedicated Annals inbox. Use Conatus as that inbox's
   only scheduled driver.
3. Read `conatus status` and the selected `want show ID` or `decision show ID`.
   Verify the intended intake, queue, work and interpretation receipts separately.
   The cursor proves intake coverage; it does not prove interpretation.
4. Stop after a feed, enqueue or inbox failure. The partial update report is
   retained and no successor stage starts. Resolve the failure before continuing.
   Do not treat a duplicate work, no new revision or process exit as proof of
   examination. A committed domain result can survive later runtime failure.

Run `conatus pause` to gate subsequent updates. An active update finishes, and
explicit retry or re-examination remains available. Run `conatus resume` only
when ordinary processing should resume. Neither command pauses the Annals inbox,
disables Clockwork or cancels admitted work.

## Recover failed processing

1. Inspect `conatus status` and the selected record. Preserve captured sources,
   cursor and receipts. Distinguish failed handoff from failed interpretation.
2. Run ordinary `conatus update` after dependency recovery for pending intake
   and queued work. A failed Annals attempt requires explicit bounded retry.
3. Pause the selected Annals inbox with its configured executable and catalog:

   ```sh
   annals library NAME inbox pause
   annals library NAME inbox status
   ```

   Verify no active processing job. Conatus' local pause is a separate gate.
4. Select the inclusive interval of failed Annals job IDs in failed
   delivery-completion order and run:

   ```sh
   conatus retry --from ANNALS_JOB_ID --through ANNALS_JOB_ID
   ```

   Inspect the retry receipt. Follow a halted event through the selected Annals
   `inbox retry status` and `inbox retry continue` interfaces. Stop if eligibility,
   result or continuation authority remains uncertain.
5. Resume the selected Annals inbox after verified recovery when ordinary
   dispatch should continue. Restore only the operator controls intended before
   the operation.

Use `conatus reexamine INTAKE_ID` only for one source already retained in Annals
when a fresh interpretation and valid application are intended. Inspect its
Annals domain receipt. Do not re-enqueue retained bytes to request examination,
create an open-ended retry or infer failure from runtime status alone.

## Replace library instructions

1. Read `conatus instructions show` and preserve the selected document for
   recovery.
2. Supply the intended exact nonblank UTF-8 document:

   ```sh
   conatus instructions set --file instructions.md
   ```

   This selects a new Annals instruction revision. It starts no model and
   rewrites no history.
3. Read `conatus instructions show` to verify the selected bytes. Re-examine
   exact retained inputs separately only when explicitly intended.

## Edit the initial Bazaar selection

1. Read `cell.prompts.conatus` through Bazaar's supported `get` interface and
   preserve its complete selection. Use an explicit
   `bazaar --database /absolute/private/bazaar.sqlite3` prefix when Conatus uses
   `CELL_BAZAAR_DATABASE`.
2. Append intended component text:

   ```sh
   bazaar update PROMPT_ID --file /absolute/prompt.txt
   ```

   Record the returned immutable version. Keep private text out of logs.
3. Publish a complete selection that pins every component to its positive
   integer version:

   ```sh
   bazaar update cell.prompts.conatus --file /absolute/selection.json
   ```

   Its content is `{"schema_version":1,"entries":{"PROMPT_ID":VERSION}}`.
   A text append alone does not change selection. Preserve migration selection
   version 1 and all referenced text versions.
4. Read the selected exact versions before new initialization. Existing library
   revisions and retained work stay unchanged. To roll back new selection,
   append the prior complete selection content; retain historical versions.

## Install or recover a release

1. Read `conatus-install inspect` and preserve the current release and both
   binding selections. Establish maintenance and drain when admitted work cannot
   tolerate replacement. Use shared coordinated deployment for coupled products.
2. Select a previously validated candidate program and its matching complete
   provider bundle.
3. Install with the expected current selection:

   ```sh
   conatus-install install --binary ABS_BINARY --bundle ABS_BUNDLE --expected-current absent
   ```

   Use the observed `releases/HASH` instead of `absent` for upgrade. Packaging
   commands accept `--home ABS_HOME`. Installation selects programs and published
   documentation only; it initializes no runtime state or schedule.
4. Run `conatus-install inspect` to read selection metadata. Installation performs
   no artifact-integrity, persistent-state-integrity, or dependency-readiness checks.
5. Run `conatus --register-usage` to register the installed commands. Product
   config, status, and dependency diagnostics remain separate operations.

Recover only to an exact retained release:

```sh
conatus-install recover --release ABS_RELEASE --expected-current releases/HASH
```

Inspect the selected release metadata and preserved bindings. Recovery does not revert the database, Annals library or Clockwork
selection. Older releases can ignore want archive state; stop if that would
violate the required active-want filtering.

## Prepare and activate one exact schedule

1. Establish a selected verified release, initialized state and ready dependencies.
   Preserve each prior binding digest, enabled intent and existing incident.
   For daily email, establish Email readiness and authority for its exact personal
   digest content. Preview with `conatus email preview` before activation.
2. Generate a new definition under an existing output parent:

   ```sh
   conatus-install schedule-definition --state-dir ABS_STATE --output ABS_NEW_FILE
   ```

   Add `--daily-email` for `conatus/daily-email`. The update key is
   `conatus/update`, every 300 seconds with run-at-load. Email is local 09:00,
   no run-at-load and a 180-second limit. Both skip overlap and select
   `halt-until-approved`. Generation writes the definition and private log
   directory; it does not register or activate it.
3. Register the definition and retain its digest:

   ```sh
   clockwork definition register ABS_NEW_FILE
   ```

4. Switch only the intended binding after authority and readiness are established:

   ```sh
   clockwork binding switch KEY DEFINITION_DIGEST
   clockwork binding show KEY
   clockwork history KEY --limit 20
   ```

   Selection enables scheduling. Update can request run-at-load. Do not run old
   and new inbox drivers together. Inspect Conatus receipts for domain success;
   Clockwork history proves activation and process outcome.
5. Disable activation with `clockwork binding disable KEY` when intended.
   Product pause does not disable either binding.

## Recover a scheduled failure

The current update or send stops on failure. Clockwork permits later scheduled
activations before the shared service-health threshold establishes a halt. Read
`conatus.service` for the threshold and pending-episode rules. Product retry and
uncertain-submission recovery remain separate from future scheduling.

1. Read `clockwork incident list KEY` and `clockwork incident show INCIDENT_ID`.
   Inspect the retained Conatus update report or email occurrence.
2. Resolve the cause and inspect domain receipts. For uncertain email acceptance,
   follow `conatus.digest.email` before any explicit retry. Scheduling continuation
   does not authorize another submission or another model attempt.
3. Obtain explicit approval for continuation and run:

   ```sh
   clockwork binding resume KEY INCIDENT_ID
   ```

4. Verify the incident and binding state and subsequent product outcome. Resume
   affects only that scheduling incident. Deployment, definition switches,
   Conatus resume and Annals recovery do not clear it.

## Operate owned maintenance

Use this route for an attended product operation. The coordinated deployment
route below acquires its own holds; do not pre-acquire a foreign hold for it.

1. Read `conatus --json config`, `conatus maintenance status` and both Clockwork
   binding selections. Preserve product pause, each present binding digest,
   enabled intent and existing incident. An absent binding remains absent.
2. Hold admission under the operation's exact owner and drain admitted commands:

   ```sh
   conatus maintenance hold OWNER
   conatus maintenance drain
   ```

   Holds survive interruption. Drain includes the prior runner lock. Stop on
   unresolved admission or a hold owned by another operation.
3. Perform the intended attended operation. Inspect selected release metadata,
   product configuration and the relevant dependency or domain receipts.
   After interruption, complete coherent recovery while preserving the hold.
4. Release only `OWNER` with `conatus maintenance release OWNER`. Restore only
   the captured operator controls. Preserve disabled or absent schedules and
   existing failure incidents.

## Coordinated deployment setup

Use the coordinator for coupled program changes. It captures product controls
and acquires its own maintenance holds. Do not place a manual hold before asking
it to execute; another operation's hold blocks admission.

1. Read `nucleus manual` for the shared Cell deployment procedure. Inspect
   `conatus --json config`, `conatus maintenance status` and both binding
   selections. Preserve configuration, product pause and present binding intent.
   Resolve any interrupted owning operation before starting another deployment.
2. Supply absolute `state_dir`, `decisions_config`, `annals_state_dir` and the
   intended `library` when defaults are unsuitable. Fresh defaults use Conatus
   state, installed Annals state and `decisions/config.toml`, and library
   `conatus`. Existing library selections cannot change during deployment.
3. Supply update `enabled` and `daily_email_enabled` separately only when a
   change is intended. Omission preserves prior intent. An absent binding remains
   absent unless its enabled setting is supplied.
4. Plan and execute from the Cell root, using the shared procedure for supplied
   settings and retained operation identity:

   ```sh
   ./deploy.sh plan conatus
   ./deploy.sh conatus
   ```

   Review the plan before execution. The coordinator holds admission, suspends
   selected bindings, drains admitted commands, selects programs, and initializes
   absent state or rebinds the final Annals executable. Existing library IDs,
   cursor, records and instructions are preserved.
5. Inspect the retained operation evidence. Verify coherent selected release and
   configuration. Verify each present or explicitly requested binding selected
   the exact new definition. An omitted absent binding needs no new definition.
   Activation restores captured pause and independent enabled intent only after
   all holds release. Existing incidents remain halted.
6. Follow coordinator recovery after interruption. Complete coherent configuration
   before its own hold release. Stop on foreign holds, incoherent configuration,
   unresolved admission or uncertain domain results; do not release another
   operation's hold to bypass refusal.

No installation, registration, catalog publication or successful activation
alone proves retention, interpretation or inbox delivery. There is no promised
activation delay, queue-drain deadline, storage capacity or completion time.

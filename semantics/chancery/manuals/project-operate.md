# Operate Semantics projects and service

Installation places resources and performs setup under the documented maintenance
boundary. It does not gate completion on persistent-state validation, artifact
integrity audits, or runtime readiness checks. The product's ordinary diagnostics
and runtime checks remain available separately.

Use this procedure to install, verify, register, seed, move, pause, retire,
diagnose, recover, or uninstall Semantics. Semantics owns project state and
semantic commits. Annals owns source bytes and feed identity; Nucleus owns
execution; Clockwork owns activation and incident halts.

Read `chancery resolve semantics.project.operate` for this procedure and its
required contracts. Use `chancery show ID` for one subject:

| Feature | Detailed contract |
| --- | --- |
| Terminology, effects, output, and replay | `semantics.repository.explore` |
| Participation, activation, lifecycle, and bootstrap seed | `semantics.projects` |
| Intake, instructions, restricted jobs, commits, and retry | `semantics.reconciliation` |
| Readiness, holds, installation guarantees, and recovery | `semantics.service` |

Read `nucleus manual` before shared maintenance. A contract or successful
readiness observation does not authorize installation, a new model job, upstream
mutation, or data removal by itself.

The `semantics` CLI prints plain text by default. Pass `--json` for the existing
command-specific JSON schema. The typed client and pinned worker explicitly
request JSON. Output selection changes no records, effects, or exit statuses.

## Inspect before effects

1. Select the exact installation, private database, and Annals decisions config.
2. Inspect project state, retained intake, selected schedule, and runtime history.
3. Verify Annals exchange 2, Nucleus execution 3, Clockwork schedule 4, and the
   complete Bazaar prompt selection for the intended work.
4. Preserve existing project pauses, schedule intent, incident halts, and holds.
5. Stop if ownership, dependency compatibility, admitted jobs, or recovery state
   cannot be proved. Do not clear a correlation, cursor, marker, or foreign hold.

```sh
semantics project list
semantics intake status
semantics --json doctor
clockwork --json binding show semantics/worker
clockwork --json history semantics/worker --limit 20
```

Doctor must report `ok:true` and green database, participation-marker, Annals-feed,
and Nucleus-reconciliation checks. This verifies selected prerequisites rather
than a future semantic result. Inspect Conversations only for historical jobs
whose retained recovery requires its contract.

## Install or update

Use `cell-ci submit COMMIT` for ordinary delivery. For a separately authorized
manual installation, select the binary, installer, provider bundle, and Clockwork:

```sh
semantics-install install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE --clockwork ABSOLUTE_CLOCKWORK
```

The installer holds admission and the worker lock, suspends owned scheduling,
backs up SQLite and sidecars, initializes or migrates state, publishes selectors,
and restores captured activation intent. It does not run doctor, replay the
Annals feed, check Nucleus readiness, or audit installed artifact bytes.

Run `semantics --register-usage` after installation. Use `semantics --json doctor`
separately when runtime diagnosis is wanted. Doctor retains its normal checks;
its result is not an installation condition. Preserve retained transactions if
resource setup, migration, publication, or recovery fails.

## Hold and drain coordinated deployment

```text
semantics --database DATABASE --json maintenance status
semantics --database DATABASE --json maintenance hold RUN_ID
semantics --database DATABASE --json maintenance release RUN_ID
```

1. Acquire the deployment run's own hold and inspect its ownership.
2. Drain live commands and separately prove durable intake and Nucleus jobs settled.
3. Let the product adapter configure with scheduling disabled.
4. Release only this run's hold after group configuration.
5. Activate only captured enabled intent after all affected holds are released.

A hold fences reads and doctor before SQLite access. Only controlled installer
commands may use `CELL_DEPLOYMENT_RUN_ID` under the same sole drained hold.
Waiting does not cancel or retry work. An unavailable existing Nucleus runtime
does not prove an empty job inventory. The candidate cannot fence unsupported
old commands: use the documented compatibility release and quiescence procedure
before coordinated deployment. Preserve other markers, pauses, and incident halts.

Settings accept only boolean `enabled`; omission preserves captured intent and
new schedules default to enabled. `{"semantics":{"enabled":false}}` keeps
scheduling disabled. Recovery restores prior intent and ignores that override.

## Register and seed a folder

1. Add exactly one `Semantics-Project: project-id` line to the regular root
   `AGENTS.md`. Explain that Semantics owns terminology and history while project
   source, tests, and product documentation own runtime behavior.
2. Register the exact canonical folder. Registration excludes earlier documents
   by capturing the current Annals watermark.
3. Seed existing vocabulary only while HEAD is revision 0.
4. Verify project identity, root, and repository HEAD.

```sh
semantics project register project-id /absolute/project/root
semantics repository seed-markdown project-id /absolute/project/root/seed.md
semantics repository show project-id
```

For a single definition, use `semantics repository seed PROJECT --label LABEL
--meaning MEANING [--grounding STATEMENT]`. Seed files must be inside the canonical
root. After verifying HEAD, the source may be removed under normal project
file-change authority. Replay uses committed effects and never reopens it.
If authorized to begin reconciliation now, run `semantics --json intake run`
and inspect its report and durable intake before relying on periodic activation.

## Move, pause, resume, or retire

Pause before semantic maintenance. Prepare the exact marker at a new root before
moving:

```sh
semantics project pause PROJECT
semantics project move PROJECT /new/canonical/root
semantics project resume PROJECT
semantics project show PROJECT
```

Verify stable identity, new canonical root, preserved HEAD/cursors, and intended
status. Pause rejects late commits and resume revalidates the marker. Neither
operation releases a deployment hold or Clockwork incident halt.

Use `semantics project retire PROJECT` only for intended permanent retirement
from paused state. It closes only unstarted pending/paused intake with zero
attempts and no retained request. Attempted, processing, failed, correlated, or
awaiting-review intake blocks the transition. A refusal leaves all state unchanged.
The former folder may be absent. Verify retired status and retained history.

## Inspect and recover intake

1. Read both collections from `semantics intake status`, including the exact
   source, state, error, retained requester/job, and any applied revision.
2. Inspect the exact Nucleus job before deciding whether another attempt is safe.
3. Use `semantics intake assign EVENT PROJECT` only for historical unassigned
   intake after verifying target identity and marker.
4. Use `semantics intake retry EVENT` only for failed intake whose prior admitted
   job is positively terminal. Stop on an active or uncertain job.
5. Inspect the next authorized worker report and retained intake/repository result.

```sh
semantics intake status --status failed
nucleus jobs status JOB_ID
semantics intake retry EVENT_ID
```

A no-effect result is interpreted completion as `ignored` without a revision.
A runtime failure after a commit preserves that commit. Do not rerun work solely
because output or transport is missing. Detailed intake and receipt rules are in
`semantics.reconciliation`.

A scheduled failure ends the current invocation. Clockwork permits later
activations before the shared service-health threshold establishes a halt.
Read `semantics.service` for the threshold and pending-episode rules.

For an established scheduling halt, inspect `clockwork incident list semantics/worker` and
`clockwork incident show INCIDENT_ID`. Resolve the product failure first. Only
explicit approval followed by `clockwork binding resume semantics/worker
INCIDENT_ID` releases that halt; it creates no domain retry. Do not clear it
through deployment, project resume, or a definition switch.

## Update selected instruction text

1. Import the reviewed migration seed before deploying callers; runtime reads
   do not initialize prompt state.
2. Append the component with `bazaar update PROMPT_ID --file /absolute/prompt.txt`.
3. Read the returned positive version and publish a complete selection with
   `bazaar update cell.prompts.semantics --file /absolute/selection.json`.
4. Retain selection version 1 and all referenced component versions.

Use `bazaar --database /absolute/private/bazaar.sqlite3` when the caller uses
`CELL_BAZAAR_DATABASE`. A text append alone does not select it. To roll back,
append the prior complete selection content. Keep text out of logs. Saved
requests and retries retain their frozen instructions.

## Activate a migrated database once

1. Stop legacy lifecycle append and capture its final opaque Decisions watermark.
2. Advance every non-retired project's legacy scan cursor to that exact value.
   Finish pending/processing legacy work. Prove each retained job terminal with
   its exact request, or positively absent if never recorded as admitted.
3. Hold external Krisis and Annals lifecycle gates. Disable the worker and public
   command, prove SQLite closed, and privately back up database plus sidecars.
4. With the dedicated Annals library healthy and Krisis still gated, invoke the
   validated installer with the captured watermark and retained maintenance:

```sh
/absolute/path/to/semantics-install install \
  --binary /absolute/path/to/semantics \
  --bundle /absolute/path/to/cell/semantics/chancery \
  --clockwork /absolute/path/to/clockwork \
  --final-decisions-watermark OPAQUE_CURSOR \
  --keep-maintenance
```

The flags assert stopped legacy append and held external gates. The candidate
checks those legacy conditions and binds one Annals library/current watermark to
all non-retired projects atomically. It imports no historical Decisions rows.
There is no default activation. Ordinary later updates omit the watermark.

5. Inspect the selected worker binding and restore captured scheduling intent. Enable Krisis last after cross-product readiness.
6. Release only the authenticated Semantics hold with a successful invocation of
   that same installed release, omitting both cutover options:

```sh
"$HOME/Library/Application Support/Semantics/install/current/package/install" install \
  --bundle "$HOME/Library/Application Support/Semantics/install/current/share/chancery/semantics" \
  --binary "$HOME/Library/Application Support/Semantics/install/current/libexec/semantics" \
  --clockwork "$HOME/.local/bin/clockwork"
```

Before the first new account, failure can restore the exact pre-cutover bytes,
selectors, and scheduler. After any new account or account-derived revision
commits, recover forward under maintenance; never run an old binary or discard
new state. Stop on an unknown legacy cursor/job or changed ownership evidence.

## Recover installation or uninstall

Inspect the exact retained `install/.transaction.*/transaction.json`, saved
inventory, selector receipts, prior schedule, and authenticated hold. Use the
current candidate installer for that transaction:

```sh
/absolute/path/to/semantics-install recover \
  --transaction /absolute/path/to/Semantics/install/.transaction.EXACT \
  --clockwork /absolute/path/to/clockwork
```

A committed transaction resumes forward. A prior null selection requires
explicit `recover --forward` for the same retained transaction. It proves exact
candidate ownership and selection while gated. It never chooses a legacy
watermark. Unknown ownership, incomplete backup, foreign locks/artifacts, or
changed evidence keeps maintenance. Do not remove the gate to force progress.
Restore captured intent and release only the operation's outer holds. Follow `semantics.service` for complete rollback guarantees.

For intended removal of installed commands and scheduling:

```sh
/absolute/path/to/semantics-install uninstall \
  --clockwork /absolute/path/to/clockwork
```

Verify the owned binding is disabled and owned public CLI/provider selectors
are removed. Uninstall retains database, releases, definitions, activation
history, and logs. Retained-state deletion needs a separate destructive decision.

## Privacy and command usage

Keep private documents, repository meanings, paths, requests, and backups inside
the local boundary. Inspect only necessary evidence. Routine logs contain
counters, opaque IDs, and bounded product failures rather than source bodies,
raw dependency diagnostics, paths, prompts, credentials, or tool values.

CLI usage recording requires nonempty `CODEX_THREAD_ID`. It records identity,
time, and thread ID, not arguments, output, or outcomes. Internal calls are
excluded. Recording errors preserve command results.

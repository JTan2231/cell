# CLI contract

## Selection

Global selectors apply to every command:

```text
--repo PATH       WEAVER_REPO       default: current directory
--state-dir PATH  WEAVER_STATE_DIR  default: ~/Library/Application Support/Weaver
```

Repository and state paths must meet the command's path requirements.
Weaver rejects unsafe narrative names, symlinked authority paths, missing
authored inputs, and malformed generated-output trees.

## Commands

```text
weaver doctor
weaver submit NARRATIVE
weaver status [RUN_ID]
weaver wait [RUN_ID]
weaver cancel [RUN_ID]
weaver check NARRATIVE
weaver worker run
weaver maintenance begin [--wait-seconds N]
weaver maintenance end
```

`NARRATIVE` is one direct child of `narratives/`. Both `how-i-work` and
`narratives/how-i-work` identify that child; arbitrary paths do not.

### `doctor`

Read strict Nucleus readiness and verify the exact protocol, Codex harness,
authentication, and invocation capabilities Weaver requires. It validates or
initializes the private state directory and locks but changes no narrative or
current workflow record.

### `submit`

Validate the project, atomically record one new current workflow, start the
detached worker, print its run ID, and exit. The worker checks Nucleus readiness
before it clears prior output. An active current workflow or maintenance gate
prevents admission. A new submission can replace a terminal current workflow;
Weaver keeps no workflow archive. The detached worker reads and freezes each
stage's selected inputs immediately before it creates the Nucleus request.
`submit` itself does not send source files.

### `status [RUN_ID]`

Read the current workflow and its stage progress without waiting or changing
state. With `RUN_ID`, fail if the current record has since been replaced.

### `wait [RUN_ID]`

Wait for the selected current workflow to become terminal, then report its
result. For a nonterminal run, start a detached worker immediately and every
30 seconds until the run ends. Use `wait` to recover after a process or machine
restart. With `RUN_ID`, fail if a new run replaced the selected run.

### `cancel [RUN_ID]`

Set cancellation intent for the selected active workflow and request
cancellation of its current Nucleus stage. It starts a detached worker after
recording nonterminal intent. Repeating cancellation is safe. A terminal
workflow is not changed into another result, and cancellation never deletes
stage output.

### `check NARRATIVE`

Mechanically validate all five persisted stage files, story anchors and links,
the exact review verdict, and final-output consistency. Rebuild to run the
editorial stages again.

### `worker run`

Internal one-shot detached-child entry point. One worker holds the run lock for
a complete active pipeline. It performs every repository read and write,
embeds the selected input contents into a content-only stage prompt, persists
that exact request, recovers it when necessary, and exits when no runnable
current workflow remains. Nucleus and Codex receive no repository cwd or
filesystem tool. Manual use is safe but normally unnecessary.

### `maintenance begin` and `maintenance end`

Deployment-only coordination commands. `begin` establishes the no-new-work
gate and waits up to 60 seconds by default for the active workflow lock. The
macOS deployer supplies a longer explicit bound. `end` removes only Weaver's
maintenance marker. These commands do not stop, restart, or modify Nucleus.

## Exit behavior

Usage and selector errors exit 2. Runtime, validation, failed workflow, and
temporary Nucleus-readiness errors exit nonzero. A successful `check` reports
`PASS` or `REVISE` and exits zero. A mechanically valid `BLOCKED` result exits
3 because it contains a diagnostic rather than publishable content. `wait` and
`worker run` use the same exit 3 for a blocked terminal workflow.

## Run-owned deployment maintenance

```sh
weaver maintenance hold RUN_ID
weaver maintenance status
weaver maintenance drain
weaver maintenance ready RUN_ID
weaver maintenance release RUN_ID
```

The selected private state root contains `deployment-maintenance/`. Its durable
run-owned holds are independent of legacy operator `.maintenance`. They block
new submissions. The admitted current workflow can finish or recover through
`wait`, `worker run`, or `maintenance drain` with its exact persisted requests.
Recovery holds a shared activity guard. Legacy operator maintenance continues
to block claims. Deployment never clears it to force progress.

These commands emit JSON with `protocol_version: 1`, `holds`, `drained`,
`nonterminal_run`, `worker_active`, and `operator_maintenance`. Drain requires
no admitted operation, no current nonterminal run, and no worker run lock.
`ready RUN_ID` additionally proves the sole matching owner and exclusive
activity availability. Release changes only the named deployment hold.

The macOS deployer accepts `--expected-current absent|releases/HASH` under its
update lock. With `CELL_DEPLOYMENT_RUN_ID`, it validates the sole drained hold
and leaves the operator marker untouched throughout success and recovery.
Standalone legacy deployment retains a preexisting operator marker. Doctor
can prove held Nucleus readiness for the named deployment while normal workflow
admission remains strict. The older begin/end commands retain their legacy
meaning and do not release deployment holds.

Deployment verification checks the installed release, Nucleus readiness,
and settled maintenance status. It submits no workflow or model job and
changes no narrative output. Ordinary workflow execution retains its stage
output validation and terminal failure rules.

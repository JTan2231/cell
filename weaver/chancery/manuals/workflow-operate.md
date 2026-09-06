# Operate Weaver workflows

Weaver keeps one current workflow record. It has no resident daemon or
LaunchAgent: supported commands start detached one-shot workers in the
interactive caller's process lineage when work or recovery requires one.

## Inspect, wait, cancel, and validate

```sh
/Users/joey/.local/bin/weaver --repo <NARRATIVE_REPOSITORY> doctor
/Users/joey/.local/bin/weaver --repo <NARRATIVE_REPOSITORY> status [RUN_ID]
/Users/joey/.local/bin/weaver --repo <NARRATIVE_REPOSITORY> wait [RUN_ID]
/Users/joey/.local/bin/weaver --repo <NARRATIVE_REPOSITORY> cancel [RUN_ID]
/Users/joey/.local/bin/weaver --repo <NARRATIVE_REPOSITORY> check <NARRATIVE>
```

`doctor` checks private-state shape and the exact Nucleus protocol, harness,
authentication, and invocation capabilities Weaver requires. It may initialize
the private state directory and locks but changes no narrative or current run.

`status` is strictly read-only and never starts work. `wait` observes until the
selected run is terminal. For a nonterminal run it starts a worker immediately
and every 30 seconds, which makes it the explicit recovery entry point after a
worker crash, logout, or restart. `cancel` records intent durably before asking
Nucleus to cancel the current stage, then starts a worker to settle the run.
Cancellation does not delete stage outputs or Nucleus history.

Supplying the observed run ID to status, wait, or cancel prevents the command
from following or affecting a later replacement. Without an ID, the command
selects the sole current record. A terminal record remains current until the
next explicit submission replaces it.

`check` validates persisted files without invoking Nucleus. It proves mechanical
shape and consistency, not freshness or a repeated editorial review.

## Diagnose and recover

Start with:

```sh
/Users/joey/.local/bin/weaver --repo <NARRATIVE_REPOSITORY> doctor
/Users/joey/.local/bin/weaver --repo <NARRATIVE_REPOSITORY> status [RUN_ID]
/Users/joey/.local/bin/nucleus jobs list \
  --requester weaver --requester-id <RUN_ID>
```

Do not restart Nucleus to recover a Weaver worker. `wait` can start another
requester process and recover its exact job, while a Nucleus restart makes an
active attempt lost. Timeout, cancellation, job-identity conflict, malformed
output, and failed validation are also terminal for the current run; Weaver
does not automatically retry them.

## Deploy or update

Deployment is a separate installed-state action:

```sh
cd /Users/joey/rust/cell/weaver
./ci.sh
<TESTED_WEAVER_INSTALL> install \
  --binary <TESTED_WEAVER_BINARY> \
  --bundle /Users/joey/rust/cell/weaver/chancery
```

The Rust `weaver-install` executable is sealed with the exact tested Weaver
candidate and requires a version-matched provider bundle. Shared `cell-install`
code verifies complete immutable inventories and owns selector compensation;
Weaver owns maintenance and prototype retirement. The predecessor format-3
release remains verifiable. The deployer stages a complete content-addressed release containing Weaver, its
deployer, manifest, and version-matched Chancery provider bundle. It begins
Weaver maintenance and lets an active workflow settle before changing
selectors. It then removes only the exact superseded
`org.weaver.worker` prototype service and plist when present, switches the
installed release and command, publishes only Chancery's `providers/weaver`
selector, validates the installed CLI, and releases only the deployment-owned maintenance it established. Weaver runtime
code never calls Chancery, and installation remains useful when the Chancery
reader is absent.

A failure before commit restores the prior release, command, provider selector,
prototype plist, loaded-service state, and maintenance state. If the new release
commits but maintenance cannot end, do not edit `.maintenance` or `current.json`
by hand. End the gate through the installed CLI:

```sh
WEAVER_STATE_DIR="$HOME/Library/Application Support/Weaver" \
  "$HOME/.local/bin/weaver" maintenance end
```

Weaver current state may contain complete active input snapshots, and Nucleus
retains complete requests and raw protocol output. Preserve both private state
roots and all retained release material during diagnosis and recovery.

## Run-owned deployment maintenance

```sh
weaver maintenance hold RUN_ID
weaver maintenance status
weaver maintenance drain
weaver maintenance ready RUN_ID
weaver maintenance release RUN_ID
```

The selected private state root owns `deployment-maintenance/`. These durable
run-owned holds are independent of legacy operator `.maintenance`. They block
new submit admission, while the already admitted current workflow may finish
or recover through wait, worker run, or maintenance drain using its exact
persisted requests. Recovery owns a shared activity guard. Legacy operator
maintenance continues to block claims; deployment never clears it to force
progress.

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

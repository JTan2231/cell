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
./packaging/macos/deploy-user.sh \
  --binary "/Users/joey/rust/cell/target/release/weaver"
```

The deployer stages a complete content-addressed release containing Weaver, its
deployer, manifest, and version-matched Chancery provider bundle. It begins
Weaver maintenance and lets an active workflow settle before changing
selectors. It then removes only the exact superseded
`org.weaver.worker` prototype service and plist when present, switches the
installed release and command, publishes only Chancery's `providers/weaver`
selector, validates the installed CLI, and ends maintenance. Weaver runtime
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

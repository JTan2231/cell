# External work storage

Cell uses one external APFS volume for generated CI and release material.
The volume must have ownership enabled. The current user's private
`~/Library/Application Support/Cell/workspace.json` selects its mount path and
volume UUID. There is no environment override or internal-disk fallback.

Telete uses this shared selection for current queued CI. Its default state is
`cell/telete`; the retained Python queue remains in `cell/ci-manager`.
The queues have separate journals, refs, services, and recovery interfaces.

The `cell` directory on that volume contains the manager and broker journals,
job worktrees, diagnostics, deployment state and receipts, release bundles,
compiler targets, Cargo downloads, tool caches, and temporary files. The host
retains source and Git metadata, installed programs and service definitions,
this small storage setting, credentials, and live product state. Nucleus and
other providers retain their own runtime records; this setting does not move
their databases or change their retention rules.

## Select storage

1. Pause and settle any initialized Telete queue, then stop its installed worker
   with `telete service stop`.
   If a Python queue or service is present, settle its jobs through `cell-ci`
   and stop its service before first storage selection. Resolve deployment and
   release work and release maintenance owners through their owning interfaces.
2. Prepare a dedicated external APFS volume. Enable ownership with
   `sudo diskutil enableOwnership /Volumes/CellWork`. Formatting erases data
   and requires a separate explicit decision.
3. Select the volume from the new source package:

   ```sh
   telete storage configure --volume /Volumes/CellWork
   telete storage status
   ```

4. Initialize a fresh Telete queue with an exact acceptable commit. For the
   Python handoff, use the final `refs/ci/accepted` commit:

   ```sh
   telete init --repo /absolute/cell --accepted-baseline COMMIT
   telete install
   telete status
   telete service start
   telete resume
   ```

Use the built Telete executable for first setup. An identical valid storage
selection can be repeated; another destination is refused. Changing storage
requires attended maintenance with all owners stopped. Initialization does not
validate the selected baseline. Telete installation leaves its service stopped
and does not replace the Python manager. Read
[Telete setup and recovery](../infrastructure/telete/chancery/manuals/queue-operate.md)
before either operation.

Compiler targets, Cargo downloads, and release caches can be discarded after
their users stop. Preserve both queues' accepted refs, candidate commits,
journals, and operation evidence. Use the owning manager's worktree cleanup.
Do not delete an unresolved deployment, queued job, maintenance hold, or live product
state. A fresh queue does not retain old submission-key deduplication or job
history. Telete does not import Python records. Reuse no historical request ID
after an explicitly authorized reset.

## Retained Python setup

Python manager commands use `cell-ci`; the root wrapper now selects Telete:

```sh
cell-ci storage status
cell-ci init --repo /absolute/cell --accepted-baseline COMMIT
cell-ci install
```

These commands retain the Python manager's setup and maintenance requirements.
An installed `cell-ci install` selects that release's own bytes. Keep the Python
worker stopped while Telete owns new jobs. Read its
[queue contract](chancery/manuals/queue-operate.md) for replacement and recovery.

## Run and recover

Admission verifies the actual mounted volume, UUID, APFS, write access, and
ownership. New CI work requires at least 2 GiB free. Telete and the Python CI
and release helpers share the setting.
Compiler targets and release caches remain separate and keep their existing
serialization.

CI bodies and release compiler processes run under a macOS filesystem sandbox.
They can write only inside the external workspace, plus `/dev/null` and
`/dev/tty`. A tool that ignores the supplied temporary or cache paths fails
instead of writing to the host. Tests must keep their fixtures in the supplied
temporary directory. Python bytecode, Cargo downloads, and Clang and Swift
module caches also use external paths. Installation runs outside that build
sandbox so product installers can update their owned host programs and state.

The Python release cache override and explicit build output must stay inside the
configured workspace. Source remains on the host; private CI and deployment
worktrees are external. The source Git repository retains commit objects and
refs. The manager removes each settled job's worktree directory and Git
registration. Unresolved jobs keep their worktrees for recovery. Read the
[Python queue operation contract](chancery/manuals/queue-operate.md#protect-retained-state)
for its cleanup status and retry behavior, or Telete's operating contract for
current jobs.

A missing, replaced, full, or unwritable drive blocks work. Reconnect the same
drive before inspecting or recovering either queue. Loss during execution is not
success. Retain operation identities and use each queue's CI and deployment
recovery. Do not initialize a new queue to bypass uncertain work. Stop and
drain all owners before clearing settled working material. Caches can be
regenerated; live queue and deployment records cannot.

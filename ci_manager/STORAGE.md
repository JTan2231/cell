# External work storage

Cell uses one external APFS volume for generated CI and release material.
The volume must have ownership enabled. The current user's private
`~/Library/Application Support/Cell/workspace.json` selects its mount path and
volume UUID. There is no environment override or internal-disk fallback.

The `cell` directory on that volume contains the manager and broker journals,
job worktrees, diagnostics, deployment state and receipts, release bundles,
compiler targets, Cargo downloads, tool caches, and temporary files. The host
retains source and Git metadata, installed programs and service definitions,
this small storage setting, credentials, and live product state. Nucleus and
other providers retain their own runtime records; this setting does not move
their databases or change their retention rules.

## Select storage

1. Pause CI admission and finish or resolve all queued and active work. Stop the
   manager with `cell-ci service stop`. Resolve any active deployment.
2. Prepare a dedicated external APFS volume. Enable ownership with
   `sudo diskutil enableOwnership /Volumes/CellWork`. Formatting erases data
   and requires a separate explicit decision.
3. Select the volume from the new source package:

   ```sh
   ./ci.sh storage configure --volume /Volumes/CellWork
   ./ci.sh storage status
   ```

4. Initialize the fresh queue using the existing accepted commit:

   ```sh
   ./ci.sh init --repo /absolute/cell --accepted-baseline refs/ci/accepted
   ./ci.sh install
   cell-ci status
   cell-ci resume
   ```

The configured destination cannot be replaced by another invocation. Changing
storage requires attended maintenance with all owners stopped. Never run an old
manager against the former journal after switching. Installation replaces the
worker. Launchd starts it from installed code with output directed to `/dev/null`.
After validating storage, the worker opens its own logs on the external volume.
This avoids launchd opening removable-volume paths before the worker can start.

Old compiler targets, Cargo downloads, release caches and completed job
artifacts can be discarded after their users stop. Preserve the Git accepted
ref and candidate commits. Remove registered worktrees through Git. Do not
delete an unresolved deployment, queued job, maintenance hold, or live product
state. A fresh queue does not retain old submission-key deduplication or job
history. Reuse no historical request ID after an explicitly authorized reset.

## Run and recover

Admission verifies the actual mounted volume, UUID, APFS, write access, and
ownership. New work requires at least 2 GiB free. The manager, broker, and
release builder share the setting. Compiler targets and release caches remain
separate and keep their existing serialization.

CI bodies and release compiler processes run under a macOS filesystem sandbox.
They can write only inside the external workspace, plus `/dev/null` and
`/dev/tty`. A tool that ignores the supplied temporary or cache paths fails
instead of writing to the host. Tests must keep their fixtures in the supplied
temporary directory. Python bytecode, Cargo downloads, and Clang and Swift
module caches also use external paths. Installation runs outside that build
sandbox so product installers can update their owned host programs and state.

The release cache override and explicit build output must stay inside the
configured workspace. Source remains on the host; private CI and deployment
worktrees are external. The source Git repository retains commit objects,
refs, and linked-worktree metadata as part of source control.

A missing, replaced, full, or unwritable drive blocks work. Reconnect the same
drive before inspecting or recovering its queue. Loss during execution is not
success. Retain operation identities and use existing CI and deployment
recovery. Do not initialize a new queue to bypass uncertain work. Stop and
drain all owners before clearing settled working material. Caches can be
regenerated; live queue and deployment records cannot.

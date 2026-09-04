# Cell CI broker

`client.py` is the repository-facing entry point. It identifies the logical
repository from Git's common directory, so linked worktrees on this host use
one broker scope rather than one scope per worktree.

The broker requires Python 3.10 or newer in addition to the product gate's
existing Rust and shell prerequisites.

Run a private gate body synchronously:

```sh
python3 ci_broker/client.py run --gate cell.root -- ./path/to/private-ci-body
```

The body runs from the worktree root unless `--cwd` is supplied. Its output is
left attached to the caller. A normal pass returns zero without adding a JSON
receipt; failures print the durable receipt. Add `--verbose-receipt` to print a
receipt on success too. `--attribution-json` can carry Vizier run, packet, and
candidate IDs without changing execution identity.

## Admission and identity

- The `heavy` lane has exactly one slot. The production `light` lane has two
  slots. Light bodies must not invoke Cargo or otherwise consume the shared
  heavy resource.
- The client streams an exact hash of tracked and untracked non-ignored source
  files. Only a Git-clean candidate can join an identical queued or running
  execution. Dirty candidates always get distinct executions. Passed results
  are not reused after an execution finishes.
- Execution identity includes host, logical repository, source, gate and gate
  version, toolchain, sanitized body environment, lane, body command and
  worktree-relative working directory, and source-check command.
- `--expected-source-key KEY` (or `CELL_CI_EXPECTED_SOURCE_KEY`) binds a child
  gate to an enclosing root plan's initial snapshot. A mismatch is reported as
  stale and returns `75` before submission. The control value is consumed by
  the client rather than added to the gate environment, so an otherwise
  identical direct call can join the same in-flight execution.
- The client points every worktree at the primary checkout's existing `target`
  directory and sets `CARGO_INCREMENTAL=0`. Heavy-lane serialization is what
  makes that shared writable target safe.
- `CARGO_BUILD_JOBS` defaults to 2. `CELL_CI_CARGO_JOBS` may override it only
  with a positive integer. The chosen value is fixed in the broker scope and is
  also part of execution identity; a conflicting caller fails closed.

The production client pins its host identity, lane configuration, and journal:
`~/Library/Application Support/Cell/ci-broker` on macOS and
`~/.local/state/cell/ci-broker` elsewhere. They are deliberately not caller
overrides: otherwise two heavy lanes could write the same shared Cargo target.
Low-level `broker.py` scope overrides are only for isolated tests whose bodies
do not use the production target. SQLite records queued, running, passed,
failed, stale, lost, and cancelled transitions durably. Old terminal data is
bounded to 256 recent executions and removed after 14 days; active and newly
finished work is never pruned.

Inspect a receipt or recover abandoned work using the same repository scope:

```sh
python3 ci_broker/client.py status --events EXECUTION_ID
python3 ci_broker/client.py recover
```

Queued and running are transient states. Final process results are: passed `0`;
failed, the body's exit code when it is 1-125 (otherwise `1`); stale `75`; lost
`70`; cancelled `130`; and broker/configuration failure `78`.

The broker is fail-closed. A body starts only after its SQLite running lease and
child PID are durable. Journal, ownership, configuration, or heartbeat failure
stops or declines the body; it never falls back to an ungoverned CI run. An
expired runner is recorded as lost, never passed.

Public `ci.sh` entry points always invoke this client. Root and release plans
invoke those public entry points, making every product gate an independently
queued unit. The shared `pipeline/ci.sh` body is an internal implementation
surface, not an admission bypass promised to callers.

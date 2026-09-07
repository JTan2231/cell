# Cell CI broker

Use `client.py` to access the broker from the repository. It identifies the
repository by Git's common directory. Linked worktrees on the same host
therefore share one broker scope.

The broker requires Python 3.10 or newer and the product gate's Rust and shell
prerequisites.

Run a private gate body synchronously:

```sh
python3 ci_broker/client.py run --gate cell.root -- ./path/to/private-ci-body
```

Unless `--cwd` is supplied, the body runs from the worktree root. A successful
run prints one result line on stdout. `--quiet-result` omits that line for an
enclosing plan. Failures print the gate, state, exit code, and private diagnostic
file location on stderr, with a diagnostic excerpt of at most 4 KiB. Queue and
execution progress appears at most once per minute. Callers that join an
existing execution also receive an initial notice.

`--verbose` streams the body's combined stdout and stderr to stderr. A caller
that joins with this option can replay retained failure diagnostics. The broker
discards successful transcripts and cannot replay them to joined callers.
`--verbose-receipt` prints the complete JSON receipt on stdout instead of a text
result, including on success. Use both flags to keep stdout machine-readable.
These flags change presentation only. `--attribution-json` adds caller-owned
correlation metadata without changing execution identity.

The broker captures output once per execution. Owning and joined callers receive
the same failure diagnostics. Logs use mode `0600` in a private directory under
the broker state directory, outside the worktree. Each log has an 8 MiB limit.
Oversized completed transcripts retain their tail and a truncation marker.
A crashed runner can leave a bounded prefix instead.

The broker drains output in bounded chunks while it handles heartbeats and
cancellation. It flushes output before the supervising caller publishes the
terminal state. It deletes successful transcripts. Failed logs follow the
journal retention rules below. Logs contain the printed output without
automatic redaction.

## Admission and identity

- The `heavy` lane has exactly one slot. The production `light` lane has two
  slots. Light bodies must not invoke Cargo or otherwise consume the shared
  heavy resource.
- The client hashes tracked files and untracked files that Git does not ignore.
  Only a Git-clean candidate can join an identical queued or running execution.
  Dirty candidates always get separate executions. The broker does not reuse
  passed results after execution ends.
- Execution identity includes host, logical repository, source, gate and gate
  version, toolchain, sanitized body environment, lane, body command and
  worktree-relative working directory, and source-check command.
- `--expected-source-key KEY` (or `CELL_CI_EXPECTED_SOURCE_KEY`) binds a child
  gate to its root plan's initial snapshot. On a mismatch, the client reports
  stale state and returns `75` before submission. The client consumes this value
  without adding it to the gate environment. An otherwise identical direct
  call can therefore join the same execution.
- The client points every worktree at the primary checkout's `target` directory
  and sets `CARGO_INCREMENTAL=0`. The heavy lane permits one writer at a time.
- `CARGO_BUILD_JOBS` defaults to 2. `CELL_CI_CARGO_JOBS` may override it only
  with a positive integer. The chosen value is fixed in the broker scope and is
  also part of execution identity; a conflicting caller fails closed.

The production client fixes its host identity, lane configuration, and journal:
`~/Library/Application Support/Cell/ci-broker` on macOS and
`~/.local/state/cell/ci-broker` elsewhere. Callers cannot override them, because
two heavy lanes could then write to the same Cargo target. Use low-level
`broker.py` scope overrides only for isolated tests that do not use the
production target. SQLite durably records queued, running, passed, failed,
stale, lost, and cancelled transitions. Old terminal data has a limit of 256
recent executions and is removed after 14 days. Active and newly finished work
is never pruned.

Inspect a receipt or recover abandoned work using the same repository scope:

```sh
python3 ci_broker/client.py status --events EXECUTION_ID
python3 ci_broker/client.py recover
```

Queued and running are transient states. Final process results are: passed `0`;
failed, the body's exit code when it is 1-125 (otherwise `1`); stale `75`; lost
`70`; cancelled `130`; and broker/configuration failure `78`.

The broker starts a body only after SQLite durably stores its running lease and
child PID. Journal, ownership, configuration, or heartbeat failure stops or
rejects the body. The broker never falls back to a CI run outside its controls.
It records an expired runner as lost.

Public `ci.sh` entry points always invoke this client. Root and release plans
use those entry points, so each product gate has its own queue entry. The shared
`pipeline/ci.sh` body is internal. Callers cannot use it to bypass admission.

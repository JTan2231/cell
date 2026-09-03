# macOS user installation

Semantics installs one content-addressed release for the current user, one CLI
selector, one Chancery provider selector, and one immutable Clockwork
definition bound as `semantics/worker`. The SQLite database, releases, and
body-free product logs are retained by uninstall.

## Prerequisites

- Installed Decisions 0.3 with `decisions.lifecycle.consume` contract 1.
- Installed Conversations 0.3 with `conversations.history.explore` contract 3
  and exact thread-summary cwd lookup.
- A healthy Nucleus service satisfying `nucleus.execution.operate` contract 1
  and all capabilities checked by Semantics doctor.
- An installed Clockwork command satisfying `clockwork.schedule.operate`
  contract 1 for the same macOS user.
- Chancery for discovery and provider publication. Semantics runtime does not
  call it.

Build and validate the candidate first:

```sh
semantics/ci.sh
cargo build --release --locked --package semantics
semantics/packaging/macos/deploy-user.sh \
  --binary /absolute/path/to/target/release/semantics \
  --clockwork "$HOME/.local/bin/clockwork"
```

Deployment refuses foreign selectors, selected Clockwork definitions, or
service files. It validates the
candidate/provider version, verifies reusable releases by a canonical content
manifest, proves any selected `semantics/worker` definition is the exact
current release-owned runner and schedule. That proof is point-in-time rather
than a Clockwork compare-and-swap; the shared Semantics update lock serializes
its own deploy and uninstall, while concurrent direct same-user mutation of
the binding is unsupported and may force maintenance-gated recovery. The
deployer registers the inactive exact-release Clockwork definition, disables
the prior binding and quiesces any owned legacy LaunchAgent, removes the public
CLI during the bounded cutover, proves SQLite quiescence, and backs up the
database plus sidecars.
The content identity covers the unrendered Clockwork template and runner. Only
after that release directory and identity exist does the deployer render its
absolute release path and interpreter/runner hashes, avoiding a circular
release hash; neither `current` nor the public CLI is execution identity.
It also holds the same cross-process worker lock used by `intake run`, excluding
a long-running manual reconciliation even while SQLite is momentarily closed.
Only then does the candidate run `--json doctor` in a scrubbed environment,
which proves schema 1 and the installed Decisions, Conversations, and
Nucleus/toolset boundaries. The
deployer atomically publishes release, CLI, and provider selectors and switches
`semantics/worker` to the candidate definition digest. Any failure restores the
prior database and selectors plus the exact prior Clockwork selection and
enabled state, or the prior owned legacy LaunchAgent during first handoff, but
never both. A previously absent or disabled-null binding becomes a disabled
tombstone that may retain the candidate digest because Clockwork has no
clear-selection operation; a previously disabled selected definition is
restored exactly without transient activation. Semantics retains the exact
product release referenced by each registered immutable definition; pruning
either is a separate explicit lifecycle operation. If scheduler/database
quiescence or complete rollback cannot be proven, the deployer fails closed
while it still holds the worker flock: it retains the release-independent
maintenance gate, attempts to disable Clockwork and the legacy label, removes
public selectors, and retains the private database, prior schedule record, and
selector record for recovery. When Clockwork cannot clear a newly selected
candidate back to a prior null selection, the exact private `current` release
selector is also retained as ownership evidence; other unprovable paths remove
it. The retained gate, rather than a
claim that both scheduler cleanups succeeded, prevents domain admission.

Deploy and uninstall share one update lock, so scheduler, selector, and
database transitions cannot race each other.

## Paths

```text
~/.local/bin/semantics
~/Library/Application Support/Semantics/semantics.db
~/Library/Application Support/Semantics/install/{current,previous,releases/}
~/Library/Application Support/Chancery/providers/semantics
~/Library/Logs/Semantics/worker.{stdout,stderr}.log
```

The Clockwork definition has a 60-second interval, no run-at-load, overlap
`skip`, no timeout, exact hashes for `/bin/sh` and the release-local runner,
and a scrubbed key-free environment. The runner resolves the Semantics payload
only as a sibling in that same immutable release; it never executes through
`current` or `~/.local/bin/semantics`. The worker is one-shot and
cross-process serialized. Its product-owned logs may
contain counters, opaque IDs, and operational failures only—not decision text,
conversation or project content, prompts, credentials, or tool payloads.
Clockwork opens those paths but does not ingest their bodies.
The private, current-user-owned, mode-`0600`, non-hard-linked
`.clockwork-maintenance` marker in Semantics application support is checked by
every release-pinned runner. Deployment accepts an existing marker only with
that exact shape and never truncates it. Deployment also verifies existing
worker log files are current-user-owned regular non-hard-linked files and
restricts their mode to `0600` without truncating their contents before
Clockwork registration. It holds the marker across the binding and database
transition; uninstall or an unprovable rollback leaves it in place so a
residual activation cannot perform domain work.
The interval is a scheduling request, not a wake-up deadline. Semantics makes
no launchd availability or worker-latency promise and still provides no
wall-clock bound from an upstream lifecycle event to a semantic revision.

## Verify

```sh
~/.local/bin/semantics --json doctor
~/.local/bin/semantics project list
~/.local/bin/clockwork --json binding show semantics/worker
~/.local/bin/clockwork --json history semantics/worker --limit 20
/Users/joey/.local/bin/chancery show semantics.repository.explore
```

For a new folder, add the exact root marker, register it, optionally seed its
existing vocabulary, and run the worker once before relying on the periodic
service:

```sh
semantics project register project-id /absolute/project/root
semantics repository seed-markdown project-id /absolute/project/root/seed.md
semantics repository show project-id
semantics --json intake run
```

The project-local seed file is required only during the atomic seed. Once HEAD
is verified, it may be removed under the project's normal file-change
authority. Semantics retains the committed effects, relative source label, and
digest and does not depend on the live file for replay.

## Recovery and uninstall

Inspect doctor, `semantics intake status`, the Clockwork binding and process
history, and the body-free stderr log. Clockwork exit state does not replace
the Semantics worker report or durable intake state. Pause a project before
semantic maintenance. Use `intake retry`
only after investigating the failed event; it refuses unsafe Nucleus replay.

```sh
semantics/packaging/macos/uninstall-user.sh \
  --clockwork "$HOME/.local/bin/clockwork"
```

Uninstall disables only the owned `semantics/worker` binding, removes any
owned legacy LaunchAgent and public CLI/provider selectors, and intentionally
retains the database, releases, definitions, activation history, and logs. Deleting
retained state is a separate destructive operation and is not authorized by
the uninstaller.

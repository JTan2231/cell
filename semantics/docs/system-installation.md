# macOS user installation

Semantics installs one content-addressed release for the current user, one CLI
selector, one Chancery provider selector, and one immutable Clockwork
definition bound as `semantics/worker`. The SQLite database, releases, and
body-free product logs are retained by uninstall.

## Prerequisites

- Installed Annals with one provisioned decisions library and
  `annals.decision-account.exchange` contract 1. Its explicit config is
  `~/Library/Application Support/Annals/decisions/config.toml` by default.
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
service files. Before it executes even the candidate version command, every
existing database, WAL, shared-memory, or rollback-journal file must be a
current-user-owned, mode-`0600`, regular non-symbolic-link file with exactly one
hard link. The same preflight applies to an existing deployment-maintenance
receipt, which is invalid without its matching gate. It validates the
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
Only then does the exact candidate run `--json doctor` in a scrubbed
environment while the old release and provider selectors remain published.
Doctor captures one Annals watermark, walks bounded pages from every distinct
installed cursor until an unchanged empty page, and reads every page twice at
that fixed watermark. It rejects changed replay, identity duplication,
nonadvancement, cycles, or more than 1,000 pages from one cursor. It also proves
schema 2, Conversations, and preserved plus successor Nucleus/toolset
boundaries. The
doctor refuses to authorize the worker switch when any active or paused
project lacks the selected Annals feed identity or cursor; a pending activation
is accepted only when there is no such project. The
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
selector and authenticated hold are retained as ownership evidence. The retained gate, rather than a
claim that both scheduler cleanups succeeded, prevents domain admission.

Deploy and uninstall share one update lock, so scheduler, selector, and
database transitions cannot race each other.

## Paths

```text
~/.local/bin/semantics
~/Library/Application Support/Semantics/semantics.db
~/Library/Application Support/Semantics/install/{current,previous,releases/}
~/Library/Application Support/Semantics/{.clockwork-maintenance,.deployment-maintenance.json}
~/Library/Application Support/Annals/decisions/config.toml
~/Library/Application Support/Chancery/providers/semantics
~/Library/Logs/Semantics/worker.{stdout,stderr}.log
```

The Clockwork definition has a 60-second interval, no run-at-load, overlap
`skip`, no timeout, exact hashes for `/bin/sh` and the release-local runner,
and a scrubbed key-free environment. The runner resolves the Semantics payload
only as a sibling in that same immutable release; it never executes through
`current` or `~/.local/bin/semantics`. The worker is one-shot and
cross-process serialized. Its product-owned logs may
contain counters, opaque IDs, and bounded product-owned operational failures
only—not raw dependency diagnostics, decision text, conversation or project
content, anchors, paths, prompts, credentials, or tool payloads.
Clockwork opens those paths but does not ingest their bodies.
Successor account reconciliation ignores inherited `TMPDIR`, resolves the
canonical Darwin per-user temporary root with a scrubbed system query, and
uses only an empty mode-`0700` per-job directory whose ownership and control-tree
ancestry are proved before Nucleus admission. Unsafe reuse, symlinks, or an
`AGENTS.md`/`.git` ancestor fail closed; cleanup uses the exact path already
proved for that invocation.
The private, current-user-owned, mode-`0600`, non-hard-linked
`.clockwork-maintenance` marker in Semantics application support is checked by
every release-pinned runner. Deployment accepts an existing marker only with
that exact shape and never truncates it. A matching private
`.deployment-maintenance.json` receipt authenticates a Semantics-owned hold by
the exact `semantics/worker` key, content-addressed release ID, and Clockwork
definition digest. `--keep-maintenance` retains that pair after commit; a later
successful invocation of the same release without the option releases it
idempotently. An unreceipted pre-existing marker is external and is never
claimed or removed. Deployment also verifies existing
worker log files are current-user-owned regular non-hard-linked files and
restricts their mode to `0600` without truncating their contents before
Clockwork registration. It holds the marker across the binding and database
transition; uninstall or an unprovable rollback leaves it in place so a
residual activation cannot perform domain work.
The interval is a scheduling request, not a wake-up deadline. Semantics makes
no launchd availability or worker-latency promise and still provides no
wall-clock bound from an accepted account to a semantic revision.

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

Registration captures the current watermark from the dedicated Annals
decisions feed. When upgrading a retained schema-one database, stop legacy
lifecycle append, capture its final opaque watermark, drain every active or
paused project cursor to that exact value, and finish every pending or
processing legacy row. Every retained legacy Nucleus correlation must be
positively terminal with its exact request, or positively absent when it was
never recorded as admitted. Hold the external Krisis and Annals lifecycle
gates, then pass the captured value to the deployer:

```sh
semantics/packaging/macos/deploy-user.sh \
  --binary /absolute/path/to/target/release/semantics \
  --clockwork "$HOME/.local/bin/clockwork" \
  --final-decisions-watermark "$FINAL_DECISIONS_WATERMARK" \
  --keep-maintenance
```

Supplying `--final-decisions-watermark` (which requires
`--keep-maintenance`) explicitly asserts that legacy append
is stopped and those external gates remain held. There is no default or
automatic migrated-database activation. After disabling the worker, suspending
the public CLI, taking the worker lock, proving SQLite closed, and privately
backing up the database plus sidecars, the candidate fetches exactly one Annals
library/watermark and atomically installs it for every non-retired project only
if all asserted legacy conditions still hold. Historical Decisions rows and
terminal, awaiting-review, failed, and unassigned history are unchanged. The
candidate then completes fixed-page replay before any release/provider selector
or worker definition is switched. Omit the watermark on later schema-two
updates; an already activated database retains its exact identity and cursors.
After Krisis is enabled last and cross-product readiness is green, release the
exact authenticated hold with a successful idempotent invocation of the
installed release, omitting both cutover options:

```sh
"$HOME/Library/Application Support/Semantics/install/current/package/deploy-user.sh" \
  --binary "$HOME/Library/Application Support/Semantics/install/current/libexec/semantics" \
  --clockwork "$HOME/.local/bin/clockwork"
```

The project-local seed file is required only during the atomic seed. Once HEAD
is verified, it may be removed under the project's normal file-change
authority. Semantics retains the committed effects, relative source label, and
digest and does not depend on the live file for replay.

## Recovery and uninstall

Inspect doctor, both collections in `semantics intake status`, the Clockwork binding and process
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

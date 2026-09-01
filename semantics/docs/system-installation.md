# macOS user installation

Semantics installs one content-addressed release for the current user, one CLI
selector, one Chancery provider selector, and one 60-second LaunchAgent. The
SQLite database, releases, and body-free logs are retained by uninstall.

## Prerequisites

- Installed Decisions 0.3 with `decisions.lifecycle.consume` contract 1.
- Installed Conversations 0.3 with `conversations.history.explore` contract 3
  and exact thread-summary cwd lookup.
- A healthy Nucleus service satisfying `nucleus.execution.operate` contract 1
  and all capabilities checked by Semantics doctor.
- Chancery for discovery and provider publication. Semantics runtime does not
  call it.

Build and validate the candidate first:

```sh
semantics/ci.sh
cargo build --release --locked --package semantics
semantics/packaging/macos/deploy-user.sh \
  --binary /absolute/path/to/target/release/semantics
```

Deployment refuses foreign selectors or service files. It validates the
candidate/provider version, verifies reusable releases by a canonical content
manifest, stops the owned worker, removes the public CLI during the bounded
cutover, proves SQLite quiescence, and backs up the database plus sidecars.
It also holds the same cross-process worker lock used by `intake run`, excluding
a long-running manual reconciliation even while SQLite is momentarily closed.
Only then does the candidate run `--json doctor` in a scrubbed environment,
which proves schema 1 and the installed Decisions, Conversations, and
Nucleus/toolset boundaries. The
deployer atomically publishes release, CLI, and provider selectors, installs a
key-free plist, and asks launchd to load it. Any failure restores the prior
database, selectors, plist, and loaded-service state. If service/database
quiescence or complete rollback cannot be proven, the deployer fails closed
while it still holds the worker flock: it removes the current and public
selectors plus the installed plist, leaving any loaded job with no executable
current runner path, and retains the private database, prior-plist, and selector
record for recovery.

Deploy and uninstall share one update lock, so service, plist, selector, and
database transitions cannot race each other.

## Paths

```text
~/.local/bin/semantics
~/Library/Application Support/Semantics/semantics.db
~/Library/Application Support/Semantics/install/{current,previous,releases/}
~/Library/Application Support/Chancery/providers/semantics
~/Library/LaunchAgents/org.semantics.worker.plist
~/Library/Logs/Semantics/worker.{stdout,stderr}.log
```

The plist has `StartInterval=60`, no `RunAtLoad`, and a scrubbed runner
environment. The worker is one-shot and cross-process serialized. Its logs may
contain counters, opaque IDs, and operational failures only—not decision text,
conversation or project content, prompts, credentials, or tool payloads.

## Verify

```sh
~/.local/bin/semantics --json doctor
~/.local/bin/semantics project list
launchctl print "gui/$(id -u)/org.semantics.worker"
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

Inspect doctor, `semantics intake status`, launchd state, and the body-free
stderr log. Pause a project before semantic maintenance. Use `intake retry`
only after investigating the failed event; it refuses unsafe Nucleus replay.

```sh
semantics/packaging/macos/uninstall-user.sh
```

Uninstall stops and removes only the owned service plus public CLI/provider
selectors. It intentionally retains the database, releases, and logs. Deleting
retained state is a separate destructive operation and is not authorized by
the uninstaller.

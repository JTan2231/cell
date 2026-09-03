# Operate Semantics projects and service

## Readiness

Before installation or maintenance, verify Decisions lifecycle contract 1,
Conversations history contract 3 with exact cwd metadata, and Nucleus execution
contract 1, plus Clockwork schedule contract 1. Chancery documents these boundaries but is not called by the
Semantics worker.

Build and deploy only a green candidate:

```sh
semantics/ci.sh
cargo build --release --locked --package semantics
semantics/packaging/macos/deploy-user.sh \
  --binary /absolute/path/to/target/release/semantics \
  --clockwork /absolute/path/to/clockwork
```

The deployer owns one transaction across service quiescence, database and
sidecar backup, candidate doctor, content-addressed release selection, public
CLI/provider selectors, and the `semantics/worker` Clockwork binding. It
hashes the unrendered template into the release, renders exact absolute paths
only after that identity exists, proves any selected definition is the exact
current release-owned runner and schedule. That point-in-time proof is not a
Clockwork compare-and-swap; Semantics serializes its own lifecycle tools, and
concurrent direct same-user binding mutation is unsupported and may force
maintenance-gated recovery. It registers the candidate definition inactive,
disables the prior binding,
quiesces any owned legacy LaunchAgent, and refuses foreign or tampered
artifacts. Deploy and uninstall share an update lock. Deployment also holds the
worker's exact cross-process flock, so a manual long-running reconciliation
cannot hide between point-in-time SQLite checks, and runs candidate doctor in a
scrubbed environment. Rollback restores the exact prior Clockwork selection
and enabled state, or the prior owned legacy LaunchAgent, never both. A
previously absent or disabled-null binding becomes a disabled tombstone that
may retain the candidate digest because Clockwork has no clear-selection
operation; a previously disabled selected definition is restored without
transient activation. If
rollback cannot prove scheduler/database quiescence, the deployer retains the
release-independent maintenance gate before releasing that flock, attempts
both scheduler cleanups, and removes public selectors. When a newly selected
candidate cannot be cleared back to a prior null selection, its exact private
`current` release selector is retained as ownership evidence; other
unprovable paths remove it. Semantics retains exact release bytes for
registered definitions. Use
the retained private transaction backup, including the database,
prior schedule state, and selector record, for explicit
recovery.

The release-independent maintenance marker must be a current-user-owned,
mode-`0600`, non-hard-linked regular file. An existing marker is validated and
never truncated. Before definition registration, the deployer likewise
validates any existing worker stdout/stderr file as a current-user-owned,
non-hard-linked regular file and restricts its mode to `0600` without changing
its contents.

Verify:

```sh
/Users/joey/.local/bin/semantics --json doctor
/Users/joey/.local/bin/clockwork --json binding show semantics/worker
/Users/joey/.local/bin/clockwork --json history semantics/worker --limit 20
/Users/joey/.local/bin/chancery show semantics.repository.explore
```

Doctor must report `ok:true` and green `database`,
`participation_markers`, `decisions_lifecycle`,
`conversations_exact_cwd`, and `nucleus_reconciliation` checks. This proves
dependency readiness, not that a future semantic event will succeed.

## Register and seed a folder

Add exactly one line to the folder's regular root `AGENTS.md`:

```text
Semantics-Project: project-id
```

Also explain locally that the Semantics repository is maintained terminology
authority and that source/tests remain behavior authority. Then:

```sh
semantics project register project-id /absolute/project/root
semantics repository seed-markdown project-id /absolute/project/root/seed.md
semantics repository show project-id
```

Registration captures the current Decisions watermark; pre-registration
history is outside version-one intake. Seeding is allowed only at revision 0,
must use a source inside the canonical root, and commits one atomic revision
with a project-relative source label and digest. The seed file is required only
for that command. After verifying repository HEAD, it may be removed under the
project's normal file-change authority; replay uses committed effects and does
not reopen the source.

## Routine operation

Clockwork requests the private one-shot worker every 60 seconds with no
run-at-load, overlap skipped, no timeout, and exact release-local interpreter
and runner hashes. It serially
resumes, scans, routes, and processes at most one reconciliation. Inspect:

```sh
semantics project list
semantics intake status
semantics --json intake run
```

Use `intake assign EVENT PROJECT` only to correct an unassigned or incorrectly
routed event after verifying the exact project. Assignment history is audited.
Medium admissions wait for review; do not force them into Nucleus. A dismissal
withdraws prior groundings append-only. A review follows the stable non-retired
project already assigned to that decision before cwd routing, so confirmation
or dismissal remains attached after a project move. New admissions use exact
current cwd and the deepest current registered root.

Pause before maintenance:

```sh
semantics project pause PROJECT
semantics project move PROJECT /new/canonical/root
semantics project resume PROJECT
```

The new root must carry the exact marker. A move preserves stable identity and
both cursors. Pausing prevents pending and late in-flight proposals from
committing. Retirement is permanent, is allowed only while paused, and refuses
unresolved assigned intake.

For failed intake, inspect the error and Nucleus job first. `intake retry`
refuses a nonterminal prior job or an admitted job whose terminal state cannot
be proven. Never clear the stored correlation or manufacture a cursor.

## Privacy and logs

The worker sends Nucleus only a minimized semantic projection of one lifecycle
event and the selected repository snapshot. Lossless anchors, cursor, and
routing metadata remain in Semantics SQLite. Nucleus runs in a neutral
temporary cwd with workspace `none`, no shell, and no web. Logs may contain
counters, opaque IDs, and operational failures.
They must not contain decision statements, rationales, conversation or project
content, prompts, credentials, diffs, commands, or tool payloads.
Clockwork retains only definition, binding, schedule, and process metadata and
does not ingest those product-owned log bodies.

## Uninstall

```sh
semantics/packaging/macos/uninstall-user.sh \
  --clockwork /absolute/path/to/clockwork
```

This disables the owned Clockwork binding, removes any owned legacy LaunchAgent
and CLI/provider selectors, and retains the database, releases, immutable
definitions, activation history, and product logs. Removing retained state requires a
separate explicit destructive decision.

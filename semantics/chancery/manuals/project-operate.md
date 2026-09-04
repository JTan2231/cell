# Operate Semantics projects and service

## Readiness

Before installation or maintenance, verify Annals decision-account exchange contract 1,
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
`current` release selector and authenticated hold are retained as ownership
evidence. Semantics retains exact release bytes for
registered definitions. Use
the retained private transaction backup, including the database,
prior schedule state, and selector record, for explicit
recovery.

The release-independent maintenance marker must be a current-user-owned,
mode-`0600`, non-hard-linked regular file. An existing marker is validated and
never truncated. `--keep-maintenance` retains a Semantics-owned marker plus
private receipt bound to the exact key, release ID, and definition digest; a
later successful invocation of the same release without that option releases
only the matching hold. An unreceipted pre-existing marker is preserved and
never claimed. Before definition registration, the deployer likewise
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
`participation_markers`, `annals_decision_feed`,
`conversations_exact_cwd`, and `nucleus_reconciliation` checks. This proves
dependency readiness, not that a future semantic event will succeed. The
Annals check fails whenever an active or paused project lacks the selected
decisions-library identity or its activation and scan cursors; only a database
with no such projects may remain activation-pending.

## Activate a migrated database

Schema 2 preserves every legacy Decisions cursor, envelope, assignment,
status, revision, effect, correlation, and mailbox receipt without creating an
Annals cursor. Before the one-time feed cutover, stop legacy lifecycle append,
advance every project through its final legacy watermark, resolve active or
ambiguous legacy Nucleus jobs, engage maintenance, disable the worker and
public command, prove the database closed, and privately back up the database
plus sidecars. With the dedicated Annals library healthy and Krisis still
gated, run the deployer with the captured final Decisions watermark and
`--keep-maintenance`; it invokes the candidate's hidden
`project activate-annals` command. It binds
the exact library and one current watermark to every existing non-retired
project. Historical Decisions rows are not imported. Verify doctor and fixed
feed replay before enabling the schedule, then enable Krisis last. Finish with
a successful same-release deployer invocation without either cutover option to
release the authenticated Semantics hold.

Before the first new account, a failed cutover restores the exact database,
sidecars, selectors, and worker state. After any new account or account-derived
revision commits, recovery is forward under maintenance; never run an old
binary or discard new state.

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

Registration captures the current watermark from the exact configured Annals
decisions library; earlier accounts are outside automatic intake. Seeding is allowed only at revision 0,
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

Use `intake assign EVENT PROJECT` only to correct unassigned account intake
after verifying the exact project. Assignment history is audited. Every valid
accepted account is immediately eligible for reconciliation; there is no
confidence, disposition, review, or supersession gate. Exact authority-thread
cwd and the deepest current registered root determine ownership. Preserved
legacy Decisions intake remains visible in a separate status collection and
retains its old states and grounding meaning.

Pause before maintenance:

```sh
semantics project pause PROJECT
semantics project move PROJECT /new/canonical/root
semantics project resume PROJECT
```

The new root must carry the exact marker. A move preserves stable identity and
both Annals and legacy cursor histories. Pausing prevents pending and late in-flight proposals from
committing. Retirement is permanent, is allowed only while paused, and refuses
unresolved assigned intake.

For failed intake, inspect the error and Nucleus job first. `intake retry`
refuses a nonterminal prior job or an admitted job whose terminal state cannot
be proven. Never clear the stored correlation or manufacture a cursor.

## Privacy and logs

The worker sends Nucleus only a normalized statement/context/action/result and
occurrence projection plus the selected repository snapshot. Exact cwd is a
transient routing input and is not stored or exposed with account intake;
anchors, cursor, project assignment, and a fixed routing outcome remain in
Semantics SQLite. Nucleus runs in a neutral temporary cwd with workspace
`none`, no shell, and no web. Logs may contain counters, opaque IDs, and bounded
product-owned failures. They must not contain raw dependency diagnostics,
account statements, context, action, result, conversation or project content,
anchors, paths, prompts, credentials, diffs, commands, or tool payloads.
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

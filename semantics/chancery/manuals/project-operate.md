# Operate Semantics projects and service

## Readiness

The coordinator's `apply` phase stages and verifies immutable release files.
`configure` runs the product-owned configuration, migration and selector
transaction with its schedule disabled. `verify` checks the installed result
without starting product work. `release` removes only the named admission hold.
After every affected hold is released, `activate` restores the captured enabled
state of the current selected definition. An originally disabled binding stays
disabled. Clockwork incident halts and product pauses remain in force.

Drain returns `waiting` while admitted commands or durable Nucleus jobs remain.
It neither cancels nor retries those jobs. A completely absent Nucleus
installation with no Nucleus database has no durable jobs to drain. An
unavailable existing runtime is not treated as an empty job inventory.

Before installation or maintenance, verify Annals decision-account exchange
contract 2 and Nucleus execution contract 3, and Clockwork schedule contract 3. Chancery documents these
contracts. The Semantics worker does not call Chancery.

Build and deploy only a green candidate:

```sh
semantics/ci.sh
cargo build --release --locked --package semantics
/absolute/path/to/target/release/semantics-install install \
  --binary /absolute/path/to/target/release/semantics \
  --bundle /absolute/path/to/cell/semantics/chancery \
  --clockwork /absolute/path/to/clockwork
```

The Rust `semantics-install` binary owns the transaction that stops services,
backs up the database and sidecars, runs candidate doctor, and selects the
content-addressed release. That transaction also controls public CLI and
provider selectors and the `semantics/worker` Clockwork binding.

The installer hashes the unrendered template into the release. It renders
absolute paths after the release identity exists and verifies the selected
definition against the current release's exact runner and schedule. This check
records the selection at that time; Clockwork does not perform a compare-and-swap.
Semantics serializes its lifecycle tools. Direct concurrent changes to the
binding are unsupported and can require recovery with maintenance held.

The installer registers the inactive candidate definition, disables the prior
binding, and stops any owned legacy LaunchAgent. It refuses foreign or changed
artifacts. Deployment and uninstall share an update lock. Deployment also holds
the worker's exact cross-process flock to exclude manual reconciliation between
SQLite checks. It runs candidate doctor in a scrubbed environment.

Rollback restores the exact prior Clockwork selection
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
registered definitions. Use `semantics-install recover --transaction ABSOLUTE_TRANSACTION --clockwork ABSOLUTE_CLOCKWORK` with the retained private database, prior schedule and selector receipts. A candidate retained after a prior null selection requires explicit `recover --forward`; it verifies the authenticated exact candidate, runs scrubbed doctor and restores its binding before releasing its hold. A durably committed transaction always resumes forward. Recovery verifies the complete saved database inventory before replacing live files and never chooses a legacy watermark.

The release-independent maintenance marker must be a current-user-owned,
mode-`0600`, non-hard-linked regular file. Both the pinned worker runner and
the public command frontend honor it, fencing public work through publication
and durable commit. An existing marker is validated and
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
`participation_markers`, `annals_decision_feed`, and
`nucleus_reconciliation` checks. The database schema is 3. This proves
dependency readiness, not that a future semantic event will succeed. The
Annals check fails whenever an active or paused project lacks the selected
decisions-library identity or its activation and scan cursors; only a database
with no such projects may remain activation-pending.

## Run-owned deployment admission

```text
semantics --database DATABASE --json maintenance status
semantics --database DATABASE --json maintenance hold RUN_ID
semantics --database DATABASE --json maintenance release RUN_ID
```

The private sibling `<database>.cell-maintenance` is separate from the
installer's marker and receipt. These commands do not open, initialize, or
migrate SQLite; status leaves an absent gate absent. They return
`protocol_version: 1`, `contract_version: 1`, `holds`, and `drained`. Drain
describes participating live commands, so durable intake and dependency jobs
still require separate product-owned quiescence evidence.

A hold fences every other public CLI and typed client command before SQLite
access, including reads and doctor because opening state may migrate it.
Existing commands may settle and holds survive process exit. Hold and release
are idempotent; release removes only its named owner and preserves project
pause, activation, and cursors. IDs contain 1–128 ASCII letters, digits,
hyphens, underscores, or periods and cannot begin with a period.

Controlled installer commands set `CELL_DEPLOYMENT_RUN_ID` only for the same
sole hold with exclusive drained activity. With no hold, they use ordinary
admission. Only doctor can prove deliberately held Nucleus readiness for that
same run: runtime drain, authentication, harness, required Semantics
capabilities, and protocol remain checked. Ordinary reconciliation still
requires normal Nucleus admission.

The compiled Cell adapter invokes the Rust installer while preserving project
pause, activation and scan cursors, and captured schedule enabled booleans.
Ordinary updates omit the legacy watermark operation below. It requires
maintenance support from currently installed public binaries before effects;
unsupported old binaries need a compatibility release through the documented
deployer and quiescence procedure. A candidate gate cannot fence old commands.
Recovery invokes the retained transaction for this exact owner. It restores
pre-commit state or completes a committed candidate with scheduling disabled.
A candidate whose prior null schedule cannot be restored is proved forward
through its authenticated receipt. Unknown ownership, changed evidence, or
unproved readiness keeps the outer hold.

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

Each accepted document after a project's activation cursor becomes intake for
that project. Semantics supplies the complete text and project repository to
its reconciliation agent. No source metadata, conversation lookup, document
layout, or mechanical relevance rule is required. The instructions in
`document-reconciliation.md` govern relevance and interpretation. The agent
can submit an empty effect list; this completes intake as `ignored` without
adding a repository revision. Each document can require one call per project,
while the worker remains serial and handles at most one reconciliation per run.

New intake IDs are local per-project identities. Embedded Annals event and
document IDs remain unchanged. Grounds, when supplied, name the original
library/event/document. Historical account and Decisions intake retain their
old projections, states, grounding kinds, and job decoders. Use `intake assign
EVENT PROJECT` only to resolve historical unassigned intake after checking the
project; assignment history is audited.

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

The worker sends Nucleus the full accepted document and selected repository
snapshot. The document may contain private conversation text. Semantics stores
it for durable replay. New intake requires no origin anchor or resolved cwd.
Nucleus runs in a neutral temporary cwd with workspace `none`, no shell, and no
web. Logs may contain counters, opaque IDs, and bounded
product-owned failures. They must not contain raw dependency diagnostics,
account statements, context, action, result, conversation or project content,
anchors, paths, prompts, credentials, diffs, commands, or tool payloads.
Clockwork retains definition, binding, schedule, process, and bounded incident metadata and
does not ingest those product-owned log bodies.

## Uninstall

```sh
/absolute/path/to/target/release/semantics-install uninstall \
  --clockwork /absolute/path/to/clockwork
```

This disables the owned Clockwork binding, removes any owned legacy LaunchAgent
and CLI/provider selectors, and retains the database, releases, immutable
definitions, activation history, and product logs. Removing retained state requires a
separate explicit destructive decision.

Rust callers may use `semantics::api::Client` for the public project, seeding,
intake, and diagnostic CLI operations with provider-owned return types. The
client does not invoke hidden cutover or worker operations. Each explicit
method retains the corresponding command's authorization and effect boundary.

The deployment adapter verifies the installed dependency configuration with
doctor. Verification does not create projects, revisions, or Nucleus jobs.

## Output selection

Project list returns ID, canonical current path, status, and HEAD. Project show
and operational receipts return complete selected records. Ordinary repository
show and search return compact terminology views. `show --provenance` returns
the full replay. Project, intake, and maintenance operations retain their
documented authority and recovery rules.

## Document compatibility

Annals exchange 2 returns complete text, filename, digest, acceptance time, and
transport identities. Pages contain at most 200 events and 4 MiB of document
bytes. A short nonempty page is not an end marker; continue until empty.
Semantics schema 3 preserves prior intake and cursor state, makes origin fields
optional, and supports per-project intake and no-change completion. Existing
admitted jobs keep their immutable request and schema identities. New jobs use
`semantics/semantic-document-reconciliation/1` with document-specific input and
result schemas. No library contents or semantic history are reinterpreted by
migration.

## Scheduled failure policy

Semantics configures Clockwork definition schema 2 for `semantics/worker` with
`[failure] on_abend = "halt-until-approved"`. A worker reports only failures
encountered by its current invocation. `intake run` prints its report and returns
nonzero when `error_event_id` is present. Retained failed intake is not rescanned
as a new incident. Normal mailbox waiting, an overlapping worker, a paused
project, maintenance, or no eligible intake does not itself constitute an abend.
A returned dependency or reconciliation error is an abend even when its job
remains in progress awaiting definitive recovery evidence.

A terminal failed or cancelled Nucleus job after a semantic commit preserves
that commit and reports its exact job ID to Clockwork. The report contains a
bounded code and opaque identity, never document text or raw runtime diagnostics.
The runner forwards Clockwork's correlated activation context through its
otherwise scrubbed environment. It does not poll historical completed jobs.

Clockwork owns the durable halt, future admission, and one retained notification
through `HOME/.local/bin/email`. Inspect `clockwork incident list
semantics/worker` and `clockwork incident show INCIDENT_ID`. Only explicit
approval followed by `clockwork binding resume semantics/worker INCIDENT_ID`
releases that halt. Definition switches, deployment, project resume, and intake
retry preserve it. The last two operations remain Semantics-owned domain controls.
Scheduling continuation creates no retry and cannot authorize a new request
while a prior Nucleus job remains uncertain. Schema-one definitions acquire the
new policy only when a schema-two definition is explicitly selected.

Deployment settings accept only `enabled`. `enabled` must be a boolean.
For example, `{"semantics":{"enabled":false}}` keeps the candidate schedule
disabled after group activation. An omitted value preserves
captured intent; a new schedule defaults to enabled. Recovery to the prior
configuration preserves captured intent and ignores this override. Incident
halts and operator pauses remain in force.

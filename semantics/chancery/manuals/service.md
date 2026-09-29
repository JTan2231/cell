# Semantics service and installation guarantees

Semantics installs one content-addressed release for the current macOS user.
It owns the public command selectors, its Chancery provider selector, private
SQLite state, and the exact runner definition bound as `semantics/worker`.
Clockwork owns activation, process history, and scheduling incidents. A successful
process exit does not prove a semantic commit.

## Paths and configuration

```text
~/.local/bin/semantics
~/.local/bin/semantics-install
~/Library/Application Support/Semantics/semantics.db
~/Library/Application Support/Semantics/install/{current,previous,releases/}
~/Library/Application Support/Semantics/{.clockwork-maintenance,.deployment-maintenance.json}
~/Library/Application Support/Annals/decisions/config.toml
~/Library/Application Support/Chancery/providers/semantics
~/Library/Logs/Semantics/worker.{stdout,stderr}.log
```

`--database ABSOLUTE_PATH` or `SEMANTICS_DATABASE` selects an isolated database.
`SEMANTICS_ANNALS_CONFIG` selects an absolute alternate decisions-library config;
`SEMANTICS_ANNALS` selects an alternate executable. New document processing
requires no Conversations lookup. Historical jobs retain their original routing
and recovery contracts, including Conversations contract 4 where required.

Annals exchange 2, Nucleus execution 3 and all Semantics doctor capabilities,
Clockwork schedule 3, and the selected Bazaar prompt set are prerequisites for
their respective work. Chancery owns publication and discovery; Semantics does
not invoke its catalog at runtime. Installation remains distinct from project
registration, domain reconciliation, and live readiness.

## Installation interfaces

The product-owned installation interfaces are:

```text
semantics-install install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE --clockwork ABSOLUTE_CLOCKWORK
semantics-install recover --transaction ABSOLUTE_TRANSACTION --clockwork ABSOLUTE_CLOCKWORK [--forward]
semantics-install uninstall --clockwork ABSOLUTE_CLOCKWORK
```

Installation can retain its owned marker with `--keep-maintenance`. The explicit
one-time legacy cutover also requires `--final-decisions-watermark OPAQUE_CURSOR`.
These flags do not choose an activation automatically or authorize a retry.
Use `semantics.project.operate` for prerequisites, ordered steps, stop conditions,
and verification. The release and recovery sections below define their effects.

## Readiness observations

```text
semantics doctor
semantics --json doctor
```

Doctor returns a typed check report even when a check fails. It must report
`ok:true` with green `database`, `participation_markers`,
`annals_decision_feed`, and `nucleus_reconciliation` checks before relying on the
installed result. It checks SQLite schema 3, exact non-retired project markers,
the explicit Annals config and selected library, Nucleus health/capabilities,
and both historical and new document schemas/toolsets.

Doctor captures one fixed Annals watermark. From every distinct installed scan
cursor, it walks bounded pages through an unchanged empty page and reads each
page twice. Cycles, nonadvancement, duplicate identities, changed replay, or more
than 1,000 pages from one cursor fail. Each active or paused project must have
the selected Annals identity and activation/scan cursors. Only a database with no
such projects may report activation pending. Its first registration captures
the then-current watermark.

Doctor proves these observations, not that a future source will yield a revision.
Deployment verification creates no projects, revisions, or Nucleus jobs. An
unavailable existing Nucleus installation is not an empty durable-job inventory.
A completely absent installation with no Nucleus database has no jobs to drain.

## Serial scheduling and failure

The immutable `semantics/worker` definition requests one one-shot pass every
60 seconds, with no run-at-load, overlap `skip`, and no activation timeout.
It pins `/bin/sh` and the release-local runner by SHA-256 and uses a scrubbed,
key-free environment. The runner selects only its sibling payload in that
immutable release, never `current` or the public CLI. A cross-process worker
flock serializes scheduled and manual work. An overlap performs no domain work.

The interval is a scheduling request, not a wake-up deadline. Semantics promises
no launchd availability, worker wake-up latency, source-to-revision latency,
throughput, storage capacity, or automatic retention horizon.

Schema-two definitions select `halt-until-approved`. The worker reports only
failures encountered in its current invocation. `intake run` prints its report
and exits nonzero when `error_event_id` is present. Historical failed intake is
not rescanned as a new incident. Mailbox waiting, overlap, a paused project,
maintenance, and no eligible intake do not themselves constitute an abend.
A returned dependency or reconciliation error is an abend even if the job remains
in progress pending definitive recovery evidence.

A terminal failed or cancelled Nucleus job after a semantic commit preserves
that commit and reports its exact opaque job ID. The runner forwards correlated
Clockwork activation context through its otherwise scrubbed environment; it does
not poll historical completed jobs. Reports contain bounded product failure
codes and opaque identities, with no source text or raw runtime diagnostics.

Clockwork retains the durable halt, future admission, and one notification through
Email. Only explicit approval and `clockwork binding resume semantics/worker
INCIDENT_ID` release that incident halt. Deployment, definition switches, project
resume, and intake retry preserve it. Resume of scheduling creates no domain
retry and cannot authorize a new request while the prior job remains uncertain.
Schema-one definitions keep their historical policy until a schema-two definition
is explicitly selected.

## Run-owned command maintenance

```text
semantics --database DATABASE --json maintenance status
semantics --database DATABASE --json maintenance hold RUN_ID
semantics --database DATABASE --json maintenance release RUN_ID
```

The private durable `<canonical-database>.cell-maintenance` gate is separate
from the installer's marker and receipt. Gate identity follows the canonical
database path, including symlink aliases; a new database uses its canonical
existing ancestor. Hard-linked databases are rejected before admission.

Maintenance commands do not open, initialize, or migrate SQLite. Status leaves
an absent gate absent and returns `protocol_version: 1`, `contract_version: 1`,
`holds`, and `drained`. Drain describes participating live commands. Durable
intake and dependency jobs need separate product-owned quiescence evidence.

A hold fences every other public CLI and typed-client command before database
access, including reads and doctor because opening state can migrate it.
Existing admitted commands can settle. Hold and release are idempotent and
survive exit. Release removes only its named owner and preserves project pause,
activation, cursors, and other holds. IDs contain 1–128 ASCII letters, digits,
hyphens, underscores, or periods and cannot begin with a period. Invalid ownership
or unproved admission fails with `deployment_maintenance`.

Controlled installation sets `CELL_DEPLOYMENT_RUN_ID` only for that same sole
hold with exclusive drained access. With no hold, commands use ordinary admission.
Only doctor can prove deliberately held Nucleus readiness for the same sole run:
runtime drain, authentication, harness, capabilities, and protocol still apply.
Ordinary reconciliation requires normal Nucleus admission.

## Release ownership and transaction

The Rust `semantics-install` binary owns install, recovery, and uninstall.
Deploy and uninstall share an update lock. It refuses foreign selectors,
selected definitions, and service artifacts. Existing database, WAL, shared-memory,
rollback-journal, and maintenance-receipt files must be current-user-owned regular
files with mode `0600`, no symlink, and one hard link. A receipt requires its gate.

The installer checks candidate/provider versions and a canonical content manifest.
The staged `cell-install-v2` inventory covers payload, installer, static frontend
and worker, unrendered schedule template, and the complete provider bundle.
Bundle bytes belong to the content-addressed release and its integrity identity.
The provider selector follows `current` and rolls back with the product.
Retained format-one and format-two releases use their exact read-only legacy
verifier and preserve original bytes.

Release identity includes the unrendered template and runner. Absolute release
paths and interpreter/runner hashes are rendered after the release identity exists.
The installer registers the inactive candidate definition, checks the selected
binding against the current release's exact runner and schedule, disables the
prior binding, stops any owned legacy LaunchAgent, and suspends public selectors.
The selected-definition check is a point-in-time observation; Clockwork supplies
no compare-and-swap. Concurrent direct mutation of that binding is unsupported.

The installer holds the worker flock, proves SQLite closed, privately backs up
the database plus `-wal`, `-shm`, and `-journal`, then runs exact candidate doctor
in a scrubbed environment. The old private selector remains selected and public
work stays fenced until publication and durable commit. Candidate doctor may
initialize or migrate state. Success publishes release, CLI, and provider
selectors and selects the exact candidate Clockwork definition.

## Installer marker and retained holds

The release-independent `.clockwork-maintenance` marker must be a current-user-owned,
mode-`0600`, non-hard-linked regular file. The pinned runner and public frontend
honor it. Existing markers are validated and never truncated. Worker output files
must also be current-user-owned regular non-hard-linked files; installation sets
them to `0600` without truncation before registering a definition.

`--keep-maintenance` retains a Semantics-owned marker and private receipt bound
to the exact key, release ID, and definition digest. A later successful invocation
of the same release without that option releases only the matching hold. An
unreceipted existing marker is external and is never claimed or removed.
Uninstall or an unproved rollback retains the gate.

## Rollback and recovery

A pre-commit failure restores the captured database inventory, sidecars, and
selectors under proved quiescence. It restores the exact prior non-null Clockwork
selection and enabled state, or the prior owned legacy LaunchAgent, never both.
A previously disabled selected definition remains disabled without transient
activation. A previously absent or disabled-null binding becomes a disabled
tombstone that may retain the candidate digest because Clockwork cannot clear
selection. Semantics retains exact release bytes for registered definitions;
pruning needs a separate decision.

If rollback cannot prove scheduler/database quiescence, it keeps the maintenance
gate before releasing the worker flock, attempts both scheduler cleanups, and
removes public selectors. It retains database backup, prior schedule and selector
receipts. When a newly selected candidate cannot return to prior null selection,
it also retains the exact private `current` selector and authenticated hold as
ownership evidence. The gate prevents domain admission even when scheduler
cleanup cannot be proved.

Interrupted transactions remain in `install/.transaction.*/transaction.json`.
Recovery verifies the complete saved database inventory and hashes before replacing
live files. A durably committed transaction resumes forward. A prior null selection
requires explicit `recover --forward`: the exact authenticated candidate, retained
release, and definition are proved; scrubbed doctor runs while gated; candidate
selectors and binding are restored before releasing its hold. Recovery never
chooses or repeats a legacy activation watermark.

An interrupted installer lock is reclaimed only when its private owner record
identifies a process proved absent. Foreign locks, changed evidence, foreign
artifacts, and unknown ownership stop recovery with maintenance retained.
Successful backups remain in `backups/deployments/`; `last-update.json` records
the installation receipt. Program rollback never authorizes discarding a committed
semantic result.

## Persistent compatibility and feed cutover

Migration 1-to-2 adds Annals identity/cursors and account intake, assignment,
correlation, and mailbox state without populating Annals cursors or changing
legacy rows. Retained prerelease schema-two account cwd records are normalized
without changing that schema version; historical state is preserved.
Migration 2-to-3 preserves intake, correlations, receipts, cursors, revisions,
and old grounding kinds. It permits absent origin fields and per-project document
intake and adds no-change revision evidence. New document ground and immutable
tool identities are distinct; old admitted jobs retain their original schemas.

A legacy database activates only through the explicit final-watermark installation
procedure in `semantics.project.operate`. It requires stopped legacy append,
drained final cursors, settled legacy work/jobs, external gates, private backup,
and `--keep-maintenance`. It binds one exact Annals library/watermark to every
non-retired project without importing historical Decisions rows. Before the first
new account, failure can restore the exact pre-cutover state. After any new account
or account-derived revision commits, recovery is forward under maintenance;
never run an old binary or discard new state.

## Coordinated deployment and uninstall

The compiled Cell adapter uses this product transaction. `apply` stages verified
immutable files; `configure` performs configuration, migration, and selector
publication with scheduling disabled; `verify` checks without domain work;
`release` removes only the run's admission hold; `activate` restores captured
intent after every affected hold is released. Existing disabled bindings remain
disabled. Project pause and incident halts remain effective.

The adapter requires maintenance support from the installed public command before
effects. Unsupported old binaries require the documented compatibility release
and quiescence procedure; a candidate gate cannot fence an old command.
Ordinary updates preserve activation/cursors and never choose a legacy watermark.
Recovery uses the exact retained transaction, restores pre-commit state or finishes
a committed candidate with scheduling disabled, and keeps outer holds on uncertainty.

Deployment settings accept only boolean `enabled`. An omitted value preserves
captured intent; a new schedule defaults to enabled. For example,
`{"semantics":{"enabled":false}}` keeps the candidate disabled after group
activation. Prior-configuration recovery ignores the override and restores
captured intent. Drain waiting neither cancels nor retries durable jobs.

Uninstall disables only the owned binding, removes any owned legacy LaunchAgent
and public CLI/provider selectors, and retains database, releases, immutable
definitions, activation history, and logs. Deleting retained state is separately
destructive and is outside uninstall authority.

## Privacy and related procedures

State and backups can contain full accepted documents, normalized conversation,
project path history, repository meanings, and historical anchors. Product logs
contain counters, opaque IDs, and bounded product-owned failures only. They exclude
raw dependency diagnostics, source/project text, paths, prompts, credentials,
diffs, commands, and tool payloads. Clockwork retains executable/schedule/process
and bounded incident metadata; it does not ingest these log bodies.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`; it records command
identity, time, and thread ID, not arguments, output, or outcomes. Internal calls
are excluded. Recording errors preserve command results.

Read `semantics.project.operate` for installation, migration, recovery, and
verification steps, and `semantics.develop.change` for changing these boundaries.
No general service response objective or future support/deprecation window is
promised. Installed documentation does not establish runtime readiness.

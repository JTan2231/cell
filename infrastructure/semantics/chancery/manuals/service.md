# Semantics service and installation guarantees

Program publication copies the selected release into fixed regular files beneath
~/Library/Application Support/Semantics/install/runtime. Public commands use that
runtime tree; current and previous retain immutable UUID archive selections. Code
signing and runtime path identity are separate from release identity.

New Clockwork definitions use schema 3: they retain the archive release ID, root and
exact hashes, and execute the fixed runtime image. Publication precedes registration.

Before publication, deployment runs the disable transition for owned Clockwork bindings
and waits for their active processes, including an active manual run on a disabled
binding. It restores saved enabled intent after registration; a failed instruction can
leave the owned bindings disabled.

Existing history, delivery records, enabled intent and incident halts retain their
meaning. Retained definitions and wrapper bytes from before this change keep their
legacy execution paths until a new installation or definition selects the runtime image.

Semantics installs one immutable release for the current macOS user.
It owns the public command selectors, its Chancery provider selector, private
SQLite state, and the exact runner definition bound as `semantics/worker`.
Clockwork owns activation, process history, and scheduling incidents. A successful
process exit does not prove a semantic commit.

The `semantics` CLI prints plain text by default. Pass `--json` for the existing
command-specific JSON schema. The typed client and pinned worker explicitly
request JSON. Output selection changes no records, effects, or exit statuses.

## Paths and configuration

```text
~/.local/bin/semantics
~/.local/bin/semantics-install
~/Library/Application Support/Semantics/semantics.db
~/Library/Application Support/Semantics/install/{current,previous,releases/,runtime/}
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
Clockwork schedule 4, and the selected Bazaar prompt set are prerequisites for
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

Doctor is a separate runtime diagnostic, not an installation gate. It returns
a typed report with `database`, `participation_markers`,
`annals_decision_feed`, and `nucleus_reconciliation` checks. It checks SQLite schema 3, exact non-retired project markers,
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
Installation does not invoke doctor or create projects, revisions, or Nucleus jobs. An
unavailable existing Nucleus installation is not an empty durable-job inventory.
A completely absent installation with no Nucleus database has no jobs to drain.

## Serial scheduling and failure

The immutable `semantics/worker` definition requests one one-shot pass every
60 seconds, with no run-at-load, overlap `skip`, and no activation timeout.
It pins `/bin/sh` and the runtime runner by SHA-256 and uses a scrubbed,
key-free environment. The definition retains the source archive identity. The
runner selects its sibling payload in `install/runtime`. A cross-process worker
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

The abnormal attempt ends its current invocation. Clockwork permits later
scheduled activations before the shared service-health threshold. By default,
five consecutive failed read-only checks, at least 60 seconds apart, halt the
binding and make its alert eligible together. Healthy or inactive checks clear
a pending episode. Checks do not retry reconciliation or replace an uncertain
Nucleus job. Clockwork owns the resulting halt and notification through Email. Only explicit approval and `clockwork binding resume semantics/worker
INCIDENT_ID` release that incident halt. Deployment, definition switches, project
resume, and intake retry preserve it. Resume of scheduling creates no domain
retry and cannot authorize a new request while the prior job remains uncertain.
Schema-one definitions keep their historical policy until a schema-two or schema-three definition
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

The installer records release metadata without candidate/provider version or
artifact-integrity checks.
The staged `cell-install-v4` inventory covers payload, installer, static frontend
and worker, unrendered schedule template, and the complete provider bundle.
Bundle bytes belong to the immutable release.
The provider selector follows `current` and rolls back with the product.
Retained format-one and format-two release metadata remains readable.

Release identity includes the unrendered template and runner. Absolute release
paths and interpreter/runner hashes are rendered after the release identity exists.
The manual installer computes the candidate definition digest, checks the
selected binding against the current release's exact runner and schedule,
disables the prior binding, stops any owned legacy LaunchAgent, and suspends
public selectors. It publishes the runtime files before it registers and selects
the candidate definition.
The selected-definition check is a point-in-time observation; Clockwork supplies
no compare-and-swap. Concurrent direct mutation of that binding is unsupported.

The installer holds the worker flock, proves SQLite closed, records the prior
schema version, then initializes or migrates the live database through the
Semantics store. It creates no data copy. The old private selector remains selected
and public work stays fenced until publication and durable commit. Setup does
not replay the Annals feed or check Nucleus readiness. Success publishes release,
CLI, and provider selectors and selects the candidate Clockwork definition.

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

A pre-commit failure restores program selectors under proved quiescence when
the database schema is unchanged. It preserves the live database and sidecars.
A schema change requires forward recovery with the retained candidate.
Compatible program rollback restores the exact prior non-null Clockwork
selection and enabled state, or the prior owned legacy LaunchAgent, never both.
A previously disabled selected definition remains disabled without transient
activation. A previously absent or disabled-null binding becomes a disabled
tombstone that may retain the candidate digest because Clockwork cannot clear
selection. Semantics retains exact release bytes for registered definitions;
pruning needs a separate decision.

If rollback cannot prove scheduler/database quiescence, it keeps the maintenance
gate before releasing the worker flock, attempts both scheduler cleanups, and
removes public selectors. It retains the prior schedule and selector receipts. When a newly selected candidate cannot return to prior null selection,
it also retains the exact private `current` selector and authenticated hold as
ownership evidence. The gate prevents domain admission even when scheduler
cleanup cannot be proved.

Interrupted transactions remain in `install/.transaction.*/transaction.json`.
Recovery preserves live state. New transaction records contain the prior schema
version. Legacy records remain readable and require forward recovery if state
was accessed; their old data copies remain untouched. A durably committed transaction resumes forward. A prior null selection
requires explicit `recover --forward`: the recorded candidate, retained
release, and definition are proved; state remains gated; candidate
selectors and binding are restored before releasing its hold. Recovery never
chooses or repeats a legacy activation watermark.

Forward recovery proves the exact candidate definition before updating a hold
receipt. An owned transaction can adopt an absent receipt or its exact recorded
prior receipt. A changed receipt stops recovery. An external unreceipted marker
remains external: recovery neither claims it nor removes it.

An interrupted installer lock is reclaimed only when its private owner record
identifies a process proved absent. Foreign locks, changed evidence, foreign
artifacts, and unknown ownership stop recovery with maintenance retained.
`last-update.json` records the installation receipt. Completed new transaction
evidence is removed. Resolved legacy transaction directories are retained under
`install/recovered-*` without restoring or removing their data copies. Program rollback never authorizes discarding a committed
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
drained final cursors, settled legacy work/jobs, external gates, and
`--keep-maintenance`. It binds one exact Annals library/watermark to every
non-retired project without importing historical Decisions rows. Migration and
activation preserve live state. After a schema change, recover forward under
maintenance with the retained candidate. Never run an incompatible old binary
or discard committed state.

## Cell manifest command and uninstall

The Cell manifest runs `semantics-install deploy` with a schema-2 request on
stdin. The request supplies candidate and source paths, a run identity and
literal product settings. The command prepares and places release files,
initializes or migrates the database, registers the worker definition, and
selects it directly. Native write and publication locks protect those changes.

The command suspends its owned worker and waits for active processes before
publishing runtime files. It restores saved enabled intent after registration.
It does not acquire application maintenance, drain durable work, check
dependency readiness, or recover automatically.
An interrupted command can leave completed effects in place. Inspect its
retained command log and current selections before a further operation.
Explicit manual install, recovery and legacy feed cutover keep their documented
procedures. An ordinary manifest command never chooses a legacy watermark.

Settings accept only boolean `enabled`. Omission preserves the current binding's
enabled intent; a new binding defaults enabled. For example,
`{"semantics":{"enabled":false}}` selects the new definition disabled.
Project pauses, existing application maintenance, and Clockwork incident halts
remain in force.

Uninstall disables only the owned binding, removes any owned legacy LaunchAgent
and public CLI/provider selectors, and retains database, releases, immutable
definitions, activation history, and logs. Deleting retained state is separately
destructive and is outside uninstall authority.

## Privacy and related procedures

State can contain full accepted documents, normalized conversation,
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

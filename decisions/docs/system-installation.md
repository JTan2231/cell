# macOS user installation

Build and deploy with the user-owned installer:

```sh
cargo build --package decisions --release
decisions/packaging/macos/deploy-user.sh \
  --binary "$PWD/target/release/decisions" \
  --clockwork "/Users/joey/.local/bin/clockwork"
```

An explicit `--home` must be an absolute, nonsymbolic directory owned by the
current operator. Because it is rendered into both XML and TOML schedule
inputs, values containing `&`, `<`, `>`, `|`, double quote, backslash, or a
newline are refused rather than escaped ambiguously.

Deploy and uninstall serialize through the same private
`install/.update-lock` directory. Uninstall may create otherwise absent private
state/install directories only long enough to contend for that lock, then
removes them again when they remain empty.

The deployer creates a content-addressed release under
`~/Library/Application Support/Decisions/install/releases`, switches `current`
and `previous`, installs `~/.local/bin/decisions`, publishes the Decisions
Chancery provider, installs a Decisions-owned synchronous `Stop` hook at
`~/.codex/hooks.json`, and registers two immutable Clockwork definitions:

- `decisions/observer` runs at most once every 60 seconds and processes at
  most one durable completed-turn observation;
- `decisions/daily-email` runs at 09:00 machine-local time and projects the
  prior day's already-observed decisions into the daily digest.

Clockwork generates the corresponding `org.clockwork.decisions.*` user
LaunchAgents. Neither binding has `RunAtLoad`. Each definition pins the exact
Decisions release runner and `/bin/sh` by digest. Under Clockwork's contract,
the generated plist's program arguments contain only Clockwork's absolute
frontend path, private entry, and stable binding key, never the product command
or a secret; Decisions does not infer ownership from the generated plist alone.
Decisions state lives at
`~/Library/Application Support/Decisions/decisions.db`; body-free observer and
daily logs live at `~/Library/Logs/Decisions/`. Both release-pinned runners
check the private, release-independent
`~/Library/Application Support/Decisions/.clockwork-maintenance` gate before
resolving dependencies or entering Decisions domain work. An invocation that
observes a valid current-user-owned `0600` marker exits successfully without
domain work; an invalid marker fails closed. The marker does not terminate an
invocation already past that check, so database quiescence is still proved
separately before migration.

The hook receives Codex's `Stop` event JSON on standard input and runs
`decisions observe ingest` synchronously with a three-second timeout. Ingest
persists only the session/turn correlation needed for later App Server
resolution, emits the required empty JSON hook result, and performs no model
call. It does not persist the event's transcript path, working directory,
model, permission mode, or latest assistant message. The 60-second Clockwork
activation is the asynchronous boundary: the pinned runner resolves the turn through
Conversations and does any eligible classification outside the Codex turn.

Codex requires the exact non-managed hook definition to be reviewed and
trusted before it runs. After deployment, open `/hooks`, review the user-level
`Stop` hook, and trust it. The deployer never bypasses or writes Codex's hook
trust state. Canary the actual Codex surface in use, including Desktop, with
one new post-activation effectful turn and verify it in
`decisions observe status`; an installed file or a CLI canary alone does not
prove that another surface emitted the event.

Updates validate every owned selector, legacy LaunchAgent, and hook before
mutation. A legacy LaunchAgent is owned only when its complete bytes equal the
current format-2 release's rendered template and its owner and mode are the
current user and `0644`; matching only its label or runner is insufficient. A
selected Clockwork digest is treated as Decisions-owned only when
`definition show` proves the exact stable key and complete current-release
manifest: release ID/root, runner/interpreter hashes, literal context, schedule,
environment, and product log paths. Foreign or ambiguous definitions are
refused. This is a point-in-time ownership proof, not a Clockwork
compare-and-swap; the shared Decisions update lock serializes its own deploy
and uninstall, while concurrent direct same-user mutation of either binding is
unsupported and may force maintenance-gated recovery. Updates engage the
private current-user-owned mode-`0600`, non-hard-linked maintenance gate before registering
inactive definitions, disabling either binding, or touching a legacy scheduler,
and never truncate an existing valid gate. Before definition registration they
also accept existing product logs only as current-user-owned regular
non-hard-linked files and restrict their mode to `0600` without truncating
content. They then quiesce any legacy services,
suspend the public Decisions command, and wait out the hook's three-second
timeout before taking a private quiescent copy of the database plus SQLite
sidecars. Candidate doctor performs the explicit
sequential migration from schema version 1 or 2 to version 3 while that copy is
available. Version 3 preserves all prior rows and backfills retained candidate
and review lifecycle events transactionally. After the new release and
selectors are staged, deployment records the observer activation baseline
exactly once. Default activation stores the next whole Unix second, so
authority items timestamped in the cutover second are conservatively excluded.
Only after that durable cutover does it publish the hook and public command,
then switch the daily binding and the observer binding last. After all durable
state and both switches have committed, it ends rollback authority and removes
the maintenance gate. A direct Decisions
LaunchAgent and its Clockwork successor are never loaded together. A Stop
event during the short command suspension may report a hook failure but is
recovered by post-baseline reconciliation. The persisted baseline is not
advanced by redeployment, uninstall, or reinstall; activity whose authoritative
user message predates it is never reconciled into the new projection.

When restoration remains provable, candidate-doctor, migration, activation,
registration, or binding-switch failure restores selectors, any prior selected
Clockwork digest and enabled state, exact legacy plist bytes and loaded state,
hook bytes, database, and sidecars. A previously absent binding may become an
inert disabled tombstone. Clockwork's guarded disabled-selection recovery form
restores a prior disabled digest without briefly enabling the schedule. If
rollback cannot prove database quiescence or restore every captured artifact,
including when a formerly unselected binding has already been switched, it
retains the maintenance gate and private transaction backup, attempts scheduler
cleanup, and removes the public command when it can prove that removal. It does
not claim that either external scheduler was disabled. A
pre-existing `~/.codex/hooks.json` is foreign on first
install and is refused rather than merged or replaced. On update or uninstall
the live file must exactly match the prior release's owned definition; a
modified file is also refused. Hooks from other Codex config layers can coexist
because Codex loads all matching sources. Uninstall removes only an exact owned
hook and retains releases, database, logs, and the activation baseline in that
database.

Clockwork stores no credential or output body and supplies only the declared
non-secret `HOME` value to each pinned runner. Both runners then build a
scrubbed environment. They pass only `HOME`, a fixed system
`PATH`, `DECISIONS_DATABASE`, and the resolved Codex executable. Neither reads
or forwards `RESEND_API_KEY`; Decisions invokes Email's installed frontend,
which owns its credential boundary. Observer output contains only bounded
status and identifiers, never conversation or email bodies.

```sh
decisions doctor
decisions observe status
clockwork binding show decisions/observer
clockwork binding show decisions/daily-email
clockwork history decisions/observer --limit 20
clockwork history decisions/daily-email --limit 20
tail -n 100 "$HOME/Library/Logs/Decisions/observer.stderr.log"
tail -n 100 "$HOME/Library/Logs/Decisions/daily-email.stderr.log"
```

Doctor starts no send and reads no message credential. It requires the installed
Email CLI's bounded `--help` surface to expose caller-supplied
`--idempotency-key` support (Email contract v2) before the schedule can load.
Its structured output reports schema version 3 and a nested observer baseline
plus queued, processing, complete, and failed counts. Doctor does not prove
Clockwork binding state, launchd delivery, hook trust, or receipt on every
Codex surface; inspect those separately as above. Clockwork history is
process-runtime evidence only; it does not prove a Decisions observation or
delivery succeeded.

For a deliberate catch-up or diagnosis, `decisions observe reconcile` discovers
and idempotently enqueues completed effectful turns after the activation
baseline without classifying them. Repeated `decisions observe process` calls
drain one ready observation at a time. Processing rows resume first; queued
sources with a future retry time are skipped and yield to other ready work.
Queued status therefore can remain nonzero when no observation is currently
ready. These are operator interfaces, not parallel worker controls.

If diagnosis proves that one observer-deferred `TurnNotFound`-shape Stop-hook
source is permanently unavailable, explicit recovery is:

```sh
decisions observe abandon OBSERVATION_ID --source-unavailable
```

The command waits for the observation-processing lock and transactionally
closes only a queued level-0 correlation with a recorded retry time and no
bound source, job, authority, verdict, or candidate. It records audited
`complete` / `not_eligible` state, stores no caller-provided reason, emits no
lifecycle event, and leaves the baseline unchanged. The exact repeat is
idempotent. Never use it for a merely unfinished turn: those rows retain
`source_not_completed_at` and must be allowed to resolve. A bound or otherwise
changed row is refused, and later completed-root reconciliation for an
abandoned correlation fails closed. This supported serialized state repair does
not require a deployment or direct SQLite editing.

Uninstall disables only Clockwork bindings whose selected definition is proven
to be the exact current Decisions release. It refuses a foreign or ambiguous
selected definition, leaves an absent or unselected disabled tombstone alone,
and removes any exact legacy services, the exact owned hook, and selectors while
retaining the maintenance gate, Clockwork definitions and history, Decisions
releases, database, and logs:

```sh
decisions/packaging/macos/uninstall-user.sh \
  --clockwork "/Users/joey/.local/bin/clockwork"
```

Removing retained state is a separate destructive action.

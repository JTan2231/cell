# macOS user installation

Build and deploy with the user-owned installer:

```sh
cargo build --package decisions --release
decisions/packaging/macos/deploy-user.sh \
  --binary "$PWD/target/release/decisions"
```

The deployer creates a content-addressed release under
`~/Library/Application Support/Decisions/install/releases`, switches `current`
and `previous`, installs `~/.local/bin/decisions`, publishes the Decisions
Chancery provider, installs a Decisions-owned synchronous `Stop` hook at
`~/.codex/hooks.json`, and loads two LaunchAgents:

- `org.decisions.observer` runs at most once every 60 seconds and processes at
  most one durable completed-turn observation;
- `org.decisions.daily-email` runs at 09:00 machine-local time and projects the
  prior day's already-observed decisions into the daily digest.

Neither LaunchAgent has `RunAtLoad`. State lives at
`~/Library/Application Support/Decisions/decisions.db`; body-free observer and
daily logs live at `~/Library/Logs/Decisions/`.

The hook receives Codex's `Stop` event JSON on standard input and runs
`decisions observe ingest` synchronously with a three-second timeout. Ingest
persists only the session/turn correlation needed for later App Server
resolution, emits the required empty JSON hook result, and performs no model
call. It does not persist the event's transcript path, working directory,
model, permission mode, or latest assistant message. The 60-second observer is
the asynchronous boundary: it resolves the completed turn through
Conversations and does any eligible classification outside the Codex turn.

Codex requires the exact non-managed hook definition to be reviewed and
trusted before it runs. After deployment, open `/hooks`, review the user-level
`Stop` hook, and trust it. The deployer never bypasses or writes Codex's hook
trust state. Canary the actual Codex surface in use, including Desktop, with
one new post-activation effectful turn and verify it in
`decisions observe status`; an installed file or a CLI canary alone does not
prove that another surface emitted the event.

Updates validate every owned selector, LaunchAgent, and hook before mutation,
stop both owned loaded services, suspend the public Decisions command, and wait
out the hook's three-second timeout before taking a private quiescent copy of
the database plus SQLite sidecars. Candidate doctor performs the explicit
sequential migration from schema version 1 or 2 to version 3 while that copy is
available. Version 3 preserves all prior rows and backfills retained candidate
and review lifecycle events transactionally. After the new release and
selectors are staged, deployment records the observer activation baseline
exactly once. Default activation stores the next whole Unix second, so
authority items timestamped in the cutover second are conservatively excluded.
Only after that durable cutover does it publish both plists, the hook, and the
public command, then bootstrap the services with the observer last. A Stop
event during the short command suspension may report a hook failure but is
recovered by post-baseline reconciliation. The persisted baseline is not
advanced by redeployment, uninstall, or reinstall; activity whose authoritative
user message predates it is never reconciled into the new projection.

Candidate-doctor, migration, activation, or bootstrap failure restores
selectors, both plists, hook bytes, database and sidecars, and the prior loaded
service state. If rollback cannot prove database quiescence or restore every
captured artifact, it fails safe with both services stopped, the public command
disabled, and the private transaction backup retained instead of exposing an
uncertain database. A pre-existing `~/.codex/hooks.json` is foreign on first
install and is refused rather than merged or replaced. On update or uninstall
the live file must exactly match the prior release's owned definition; a
modified file is also refused. Hooks from other Codex config layers can coexist
because Codex loads all matching sources. Uninstall removes only an exact owned
hook and retains releases, database, logs, and the activation baseline in that
database.

Both runners use a scrubbed environment. They pass only `HOME`, a fixed system
`PATH`, `DECISIONS_DATABASE`, and the resolved Codex executable. Neither reads
or forwards `RESEND_API_KEY`; Decisions invokes Email's installed frontend,
which owns its credential boundary. Observer output contains only bounded
status and identifiers, never conversation or email bodies.

```sh
decisions doctor
decisions observe status
launchctl print "gui/$(id -u)/org.decisions.observer"
launchctl print "gui/$(id -u)/org.decisions.daily-email"
tail -n 100 "$HOME/Library/Logs/Decisions/observer.stderr.log"
tail -n 100 "$HOME/Library/Logs/Decisions/daily-email.stderr.log"
```

Doctor starts no send and reads no message credential. It requires the installed
Email CLI's bounded `--help` surface to expose caller-supplied
`--idempotency-key` support (Email contract v2) before the schedule can load.
Its structured output reports schema version 3 and a nested observer baseline
plus queued, processing, complete, and failed counts. Doctor does not prove
launchd state, hook trust, or receipt on every Codex surface; inspect those
separately as above.

For a deliberate catch-up or diagnosis, `decisions observe reconcile` discovers
and idempotently enqueues completed effectful turns after the activation
baseline without classifying them. Repeated `decisions observe process` calls
drain one observation at a time. These are operator interfaces, not parallel
worker controls.

Uninstall both services, the exact owned hook, and selectors while retaining
releases, database, and logs:

```sh
decisions/packaging/macos/uninstall-user.sh
```

Removing retained state is a separate destructive action.

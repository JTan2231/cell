# Krisis CLI

The public executable is `krisis`. Its default database remains
`~/Library/Application Support/Decisions/decisions.db` so existing Decisions
history migrates in place. `--database` or `KRISIS_DATABASE` selects an explicit
compatible database.

Annals delivery and doctor use three explicit values:

- `--annals-binary` / `KRISIS_ANNALS_BINARY`
- `--annals-config` / `KRISIS_ANNALS_CONFIG`
- `--annals-library-id` / `KRISIS_ANNALS_LIBRARY_ID`

Supply all three values. The binary and config paths must be absolute. The
library ID must contain exactly 32 lowercase hexadecimal characters.

## Deployment maintenance

```text
krisis --database DATABASE --json maintenance status
krisis --database DATABASE --json maintenance hold RUN_ID
krisis --database DATABASE --json maintenance release RUN_ID
```

The private durable gate is the sibling directory
`<canonical-database>.cell-maintenance`. Maintenance commands do not open, initialize,
or migrate SQLite; status leaves an absent gate absent. Their JSON result
contains `protocol_version: 1`, `contract_version: 1`, `holds`, and `drained`.
Drain reports live command admission only; the deployment adapter must also
account for durable observations and unfinished dependency jobs.

Holds prevent every other public CLI command, including typed client calls,
status, and doctor, before database access because opening Krisis state may
migrate it. Already admitted commands may settle. Repeated holds are
idempotent and survive process exit; release removes only the named owner's
hold and does not reset the observer baseline or clear another hold. Run IDs
contain 1–128 ASCII letters, digits, hyphens, underscores, or periods and
cannot begin with a period. Invalid owners and unavailable admission fail
with `deployment_maintenance`.

For an installer-owned command, `CELL_DEPLOYMENT_RUN_ID=RUN_ID` requires that
exact sole hold and exclusive access after live activity drains. It cannot
bypass another owner. With no hold, commands use ordinary admission. Doctor
can accept deliberately held Nucleus readiness only after
`health_for_deployment` proves this same run's sole Nucleus hold, runtime
drain, authentication, and harness. Product capabilities and protocol checks
still apply. Observation processing always requires normal Nucleus admission.

## Supported commands

`krisis document build --thread-id THREAD --turn-id TURN --directory DIRECTORY`
freezes the full normalized conversation through one completed exchange,
classifies that exchange, and constructs an ordinary Markdown source document.
Repeat the command to resume the same run. `krisis document render --directory
DIRECTORY` reconstructs the document from the saved result without external
services. Both commands emit JSON. They require no Annals configuration and do
not open the observer database. See [source documents](source-documents.md) for
the result format, source completeness, limits, and recovery rules.

`krisis doctor` opens and migrates the database, checks Conversations and
Nucleus readiness, and invokes Annals `decision-feed watermark` with the exact
config. It requires the standard JSON envelope and matching contract-version-2
dedicated library identity.

`krisis observe activate [--at UNIX_SECOND]` writes the post-deployment
exchange-completion baseline exactly once. With no explicit time, it conservatively uses
the next Unix second.

`krisis observe ingest` reads one Codex Stop-hook JSON object from standard
input and durably stores only its session/turn correlation.

`krisis observe process` handles one observation or pending document under the
serial worker lock. It verifies the Annals target for that work. The first
processing error marks the observation `failed`, retains the error, and removes
it from automatic selection. This includes missing or incomplete sources,
classification errors, uncertain dependency calls, and delivery errors.
A newly recorded observation failure returns nonzero with
`observation_processing_failed`. Errors that prevent a durable outcome also
return nonzero. Both cause the configured Clockwork schedule to halt.

Failed deliveries retain the exact pending document and target. Their observations
remain failed until explicit retry. Other observations and deliveries wait for
explicit Clockwork continuation when a scheduling incident is active.
Repeated hooks and reconciliation never requeue a failed observation.

`krisis observe status [--date YYYY-MM-DD]` reports baseline, queue states,
failure summaries, and pending/accepted Annals document counts without invoking
dependencies.

`krisis observe reconcile [--date YYYY-MM-DD]` discovers missed completed turns
through Conversations and enqueues correlations without classifying them.

`krisis observe retry OBSERVATION_ID` explicitly releases a failed observation
for another attempt after its cause has been inspected. Pending deliveries reuse
the same document key, bytes, and target. Uncertain Nucleus work and accepted
classifications reuse their saved run and request. A new classification attempt
is created only when no run exists or its saved terminal result has no accepted
classification. Previous failure records remain available in private state.

`krisis observe abandon OBSERVATION_ID --source-unavailable` closes only one
proven-permanently-unavailable, entirely unbound queued source. It is not a way
to clear merely unfinished or failed work.

## Worker health

Run `krisis health [--max-idle-seconds N] [--json]` to inspect the worker.
This command needs no Annals configuration and invokes no dependency. Like other
state commands, it opens and can migrate Krisis state and obeys maintenance holds.

`working` means the serial worker lock has a live owner. `idle` means no worker
owns it and the last run finished within the selected interval. Empty polls
update the last finish time without resetting how long the worker has been idle.
A newly failed observation sets the last worker error. Historical failure counts
do not create new worker errors or scheduling incidents.

The default idle limit is 180 seconds for the installed 60-second schedule.
A longer interval produces `stale`. This is a local diagnostic threshold, not a
Clockwork delivery guarantee. `error` reports the last worker error, including a newly failed observation.
`interrupted` means a started run has no recorded finish and no lock owner.
`unobserved` means this database has no worker activity record yet.

JSON contains `ok`, `state`, `checked_at`, `state_since`,
`state_duration_seconds`, `last_started_at`, `last_finished_at`,
`max_idle_seconds`, and `error_code`. Timestamps are Unix seconds; durations
are seconds at `checked_at`. An unobserved transition has null timing, including
an interrupted run whose exit time is unknown. For `stale`, `state_since` is
when the last finished run exceeded the selected idle limit. A live run's
duration measures ownership, not proof of classifier progress.

Exit zero means `working` or `idle`; other states print their report and exit
nonzero. The typed `Client::health` returns the report even when `ok` is false.
Read `observe status` for record counts and `doctor` for dependency readiness.
Failed observations are retained history for optional later investigation. Their
count does not determine worker health or require repeated alerts.

## Legacy read-only compatibility

`krisis events watermark` and `krisis events read --after CURSOR [--limit N]`
read retained Decisions lifecycle envelopes for existing consumers. Limits are
1 through 1000. Krisis does not append new events.

`krisis show DECISION_ID` reads retained legacy Decisions candidate state only;
new documents belong in Annals and are not browsable in Krisis.

The old `daily` and `review` command spellings are hidden compatibility parsers
that always fail with `legacy_surface_retired`. They perform no build, send,
email, review, or state mutation. No active daily/review schedule exists.

Use `--json` for machine-readable Krisis responses. Upstream dependency error
bodies, prompts, accounts, and transcript text are not copied into routine
error output.

Deployment gate identity follows the canonical database path (including a
symlink alias), or the canonical existing ancestor for a new database.
Hardlinked databases are rejected before admission. Maintenance status still
does not open or initialize the database.

`krisis observe status [--date YYYY-MM-DD] [--limit N]` retains global/window
counts and displays at most 20 failure IDs and codes by default. JSON includes
`failures_has_more`. To see more failures, set `--limit` to a larger positive
integer. Failure counts are
never truncated. The Rust client exposes `status_limit` for explicit selection.
This does not change observer admission, retries or the legacy event stream.

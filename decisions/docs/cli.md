# Krisis CLI

The public executable is `krisis` 0.4.0. Its default database remains
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

`krisis observe process` requires all Annals configuration values and first
verifies `decision-feed watermark`. It then delivers the oldest pending document
bound to that target. If none is pending, it resumes or classifies one
observation bound to the target. A changed config path or library identity
causes failure. Repeated calls are safe; processing remains serial.

`krisis observe status [--date YYYY-MM-DD]` reports baseline, queue states,
failure summaries, and pending/accepted Annals document counts without invoking
dependencies.

`krisis observe reconcile [--date YYYY-MM-DD]` discovers missed completed turns
through Conversations and enqueues correlations without classifying them.

`krisis observe retry OBSERVATION_ID` opens a new attempt epoch only for a
terminal observation whose cause has been diagnosed and corrected.

`krisis observe abandon OBSERVATION_ID --source-unavailable` closes only one
proven-permanently-unavailable, entirely unbound queued source. It is not a way
to clear merely unfinished or failed work.

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

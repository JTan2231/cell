# Krisis CLI

The public executable is `krisis` 0.4.0. Its default database remains
`~/Library/Application Support/Decisions/decisions.db` so existing Decisions
history migrates in place. `--database` or `KRISIS_DATABASE` selects an explicit
compatible database.

Annals delivery and doctor use three explicit values:

- `--annals-binary` / `KRISIS_ANNALS_BINARY`
- `--annals-config` / `KRISIS_ANNALS_CONFIG`
- `--annals-library-id` / `KRISIS_ANNALS_LIBRARY_ID`

The binary and config paths must be absolute. The expected library ID is exactly
32 lowercase hexadecimal characters. Partial configuration is rejected.

## Supported commands

`krisis doctor` opens and migrates the database, checks Conversations and
Nucleus readiness, and invokes Annals `decision-feed watermark` with the exact
config. It requires the standard JSON envelope and matching contract-version-1
dedicated library identity.

`krisis observe activate [--at UNIX_SECOND]` writes the post-deployment
authority baseline exactly once. With no explicit time, it conservatively uses
the next Unix second.

`krisis observe ingest` reads one Codex Stop-hook JSON object from standard
input and durably stores only its session/turn correlation.

`krisis observe process` requires the complete Annals configuration and first
verifies `decision-feed watermark`. It then performs at most one unit of work:
deliver the oldest target-bound pending account, otherwise resume or classify
one target-bound observation. A changed config path or library identity fails
closed. It is safe to invoke repeatedly and processing remains serial.

`krisis observe status [--date YYYY-MM-DD]` reports baseline, queue states,
failure summaries, and pending/accepted Annals account counts without invoking
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
new accounts belong in Annals and are not browsable in Krisis.

The old `daily` and `review` command spellings are hidden compatibility parsers
that always fail with `legacy_surface_retired`. They perform no build, send,
email, review, or state mutation. No active daily/review schedule exists.

Use `--json` for machine-readable Krisis responses. Upstream dependency error
bodies, prompts, accounts, and transcript text are not copied into routine
error output.

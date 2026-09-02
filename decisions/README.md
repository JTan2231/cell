# Decisions

Decisions is an independently versioned local product that continuously
observes completed, effectful root Codex turns and turns explicit user
settlements into a reviewable daily projection and frozen plain-text email. A
synchronous Codex `Stop` hook records only a durable turn correlation; a
single-worker observer resolves and classifies that narrow scope after the
turn, expanding to prior conversation context only when required. Decisions
reads through the `conversations` library, delegates bounded classification to
Nucleus, owns its SQLite domain state, and invokes the installed `email` CLI
for delivery. Decisions does not copy source text into its own database, but
Nucleus durably retains each exact classifier request and tool exchange under
its separate local retention policy; a level-1 expansion therefore retains the
normalized thread prefix supplied to that request.

```sh
decisions daily build --date 2026-08-31
decisions daily preview --date 2026-08-31
decisions daily send --date 2026-08-31
decisions daily abandon --date 2026-08-31
decisions observe status
decisions observe reconcile --date 2026-08-31
decisions observe process
decisions observe abandon OBSERVATION_ID --source-unavailable
decisions observe retry OBSERVATION_ID
decisions events watermark --json
decisions events read --after OPAQUE_CURSOR --json
decisions show DECISION_ID
decisions review confirm DECISION_ID
```

The installed observer runs every 60 seconds. The 09:00 service normally does
no model work: it reconciles and drains only missed observations before
projecting the previous local day as of a durable completion cutoff. A turn
completing later can enter a manual rebuild of its authority date, but does not
automatically amend an accepted scheduled delivery. The write-once activation
baseline prevents deployment from importing older messages.

A temporarily incomplete or unavailable source is deferred so other ready
observations can continue. After the exact Stop-hook source has been proven
permanently unavailable, explicit `observe abandon --source-unavailable`
recovery can close only that previously deferred, entirely unbound correlation
as audited `not_eligible`; it must never be used for a merely unfinished turn.

The append-only lifecycle stream exposes transactionally coupled candidate
admissions and reviews to local consumers through opaque replay-safe cursors.
It contains normalized decision data and stable source anchors, never
transcripts, working directories, paths, diffs, commands, or tool output.

See [docs/architecture.md](docs/architecture.md), [docs/cli.md](docs/cli.md),
[docs/data-model.md](docs/data-model.md), and
[docs/system-installation.md](docs/system-installation.md).

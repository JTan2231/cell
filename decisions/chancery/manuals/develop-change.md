# Develop Krisis

Read the nested `AGENTS.md`, query the registered decisions Semantics repository,
and resolve affected Conversations, Nucleus, Annals, and Clockwork contracts.
Keep code and tests consistent for exchange coverage, complete source bounds,
saved classification before acknowledgement, deterministic document rendering,
Annals idempotency, outbox retirement, and legacy Decisions decoding.

Run `decisions/ci.sh`. It performs no release, deployment, live state migration,
Nucleus execution, or Annals acceptance.

## Source document construction

`krisis document build --thread-id THREAD --turn-id TURN --directory DIRECTORY`
uses the same construction engine as `observe process`. The explicit command
is local construction only. It freezes the complete normalized root-task
history through one completed exchange and submits a yes/no classification
with a short summary. It writes a private `run.json` containing the exact source,
Nucleus request, and durable tool receipts. A positive verdict produces
`decision.md`: an escaped summary heading and deterministic Markdown rendering
of every captured user/assistant message. A negative verdict produces no file.
No Annals target, observer database, or consumer cursor participates.

The directory must be private. The ordinary deployment gate applies. Build
invokes Conversations and Nucleus, so unlike CI it performs model execution.
The complete encoded prompt must fit 262,144 UTF-8 bytes; larger sources fail
without truncation. The agent supplies one or two summary sentences; code checks
only field shape and a nonblank single line of at most 1,000 characters, plus
source and transport integrity. There are no truth, privacy, or raw-copy checks
on the result.

The new immutable Nucleus toolset `krisis/decision-document/1` exposes
`submit_decision` with `krisis.tool.submit-decision.input.v1` and
`krisis.tool.submit-decision.result.v1`. It uses the bounded Nucleus execution
policy with no workspace, local execution, or web access. Historical account
registrations retain their schemas and validators for old records only.

Repeat the same command and directory after uncertainty. It resumes the exact
saved request and job, acknowledges saved identical results, and refuses changed
source identity or tool-call content. Accepted classification commits before
acknowledgement and remains valid after runtime failure. Terminal failure without
a classification does not authorize an automatic successor.
`krisis document render --directory DIRECTORY` reconstructs a missing document
from saved state without external services; different existing bytes fail.
Keep the full directory for recovery. The local commands do not deliver to Annals. Automatic observation processing
records a target-bound outbox and supplies the sole active production path.

## Observation outcomes and worker health

The observer records the first returned processing error as a failed observation.
A saved conversation read failure (`document_source_unavailable`) returns zero
and permits later scheduled work. Other errors return nonzero before successor
work. Preserve error history across explicit retries.
An uncertain saved job or accepted classification must reuse its run; a failed
pending delivery must reuse its key, exact bytes, and target. A recorded
observation failure is an abend unless it is a saved conversation read failure.

Keep `health` independent of retained failures. It reports live lock ownership,
last run timing, continuous idle time, and unhandled worker errors. Empty polls
must not reset idle duration. A missing worker finish cannot prove an exit time.
Exercise first-error queue progress, repeated-hook/reconciliation behavior,
retry identity, migration, and health timing in synthetic fixtures.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.

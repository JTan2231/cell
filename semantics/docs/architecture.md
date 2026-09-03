# Architecture

## Authority

Semantics owns project registration, event routing, intake transitions,
concept identity, semantic validation, immutable revisions, and the final
SQLite transaction. Decisions owns lifecycle event order and decision/review
facts. Conversations owns exact thread metadata, including the recorded
working directory. Nucleus owns bounded model execution and mailbox transport;
its proposal is never authoritative until Semantics validates and commits it.

Project source, tests, and existing product documentation remain authoritative
for runtime behavior. The semantic repository is authoritative for maintained
terminology and its revision history.

## Flow

1. Registration canonicalizes an exact folder, verifies its root
   `AGENTS.md` marker, and captures the current Decisions watermark. Version
   one intentionally does not import earlier lifecycle events.
2. A serial one-shot worker reads immutable lifecycle pages after each
   project's durable cursor.
3. A review for an already-assigned decision follows that stable non-retired
   project directly, preserving lifecycle continuity across moves or missing
   old thread metadata. For an admission or otherwise unbound event,
   Conversations resolves the cited thread's exact working directory and the
   deepest current registered root containing it owns the event. A known cwd
   outside every root is ignored. Missing/failed exact-cwd lookup or anything
   other than one authority source remains visible as unassigned intake for
   diagnosis.
4. High-confidence admissions and effective confirmations are eligible for
   reconciliation. Medium-confidence admissions wait for review. Low
   confidence is rejected. Dismissals withdraw matching active decision
   groundings without erasing history.
5. Semantics persists one stable Nucleus job correlation, supplies only a
   minimized semantic projection of the event plus the complete selected
   repository snapshot, and exposes one immutable managed tool. Lossless source
   anchors, cursor, and routing metadata remain in Semantics SQLite.
6. The tool callback validates the base revision, sequential concept IDs,
   effect invariants, exact decision grounding, active project state, and
   replay safety before atomically appending a revision and receipt.

The Nucleus job runs in a deterministic neutral temporary directory with
workspace access `none`, no shell, and no web. It cannot read a registered
project folder. Ambiguous transport recovery reuses the same requester and job
identity; an operator may create a new attempt only after the prior job is
positively terminal.

## Serial service

Clockwork key `semantics/worker` requests one hidden `intake run` every 60
seconds, without run-at-load or an activation timeout. Its immutable definition
records the exact release ID and root and pins `/bin/sh` plus the release-local
runner by SHA-256. Semantics' release manifest and retention rules own the
sibling payload and full release integrity. The definition uses a scrubbed
environment and skips overlap. A
cross-process Semantics lock remains the authoritative serialization boundary;
an independently started overlapping invocation is a harmless no-op. Each run
resumes one processing item first, scans bounded lifecycle pages, and applies
at most one reconciliation. Pausing a project prevents a late proposal from
committing.

A release-independent, current-user-owned, mode-`0600`, non-hard-linked
maintenance marker prevents any release-pinned runner from entering domain
work during deployment, uninstall, or fail-closed recovery. Lifecycle tooling
never truncates an existing marker and refuses any other shape. A successful
deployment clears it only after binding and database state commit; uninstall
and an unprovable rollback retain it. Existing product log files must likewise
be current-user-owned regular non-hard-linked files; deployment makes their
mode `0600` without truncating content before definition registration.

Clockwork records only schedule, definition, binding, and process outcomes. It
does not inspect Semantics domain state or ingest product log bodies. A zero
process exit reports only that the one-shot invocation returned successfully;
the Semantics database and worker report remain authoritative for intake and
commit outcomes.

Service stdout contains only counters and opaque identifiers; stderr contains
operational failures. Decision statements, rationales, project content,
conversation text, diffs, commands, tool output, credentials, and Nucleus
prompts must not enter service logs.

## Failure boundaries

Lifecycle cursors advance only after an event is durably recorded or
source-derived as irrelevant. Repository revisions and typed effects are
append-only. Failed or awaiting-review intake stays explicit. Mailbox call
receipts make repeated delivery idempotent and reject conflicting replay.
Operator retry refuses a prior Nucleus job that is active or whose admitted
state cannot be established.

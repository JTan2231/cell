# Architecture

## Authority

Semantics owns project registration, account routing, intake transitions,
concept identity, semantic validation, immutable revisions, and the final
SQLite transaction. Annals owns the exact decisions-library identity, durable
acceptance order, immutable account projection, and opaque feed cursors.
Conversations owns exact thread metadata, including the recorded working
directory. Nucleus owns bounded model execution and mailbox transport; its
proposal is never authoritative until Semantics validates and commits it.
Legacy Decisions lifecycle state remains preserved and decodable, but is not
scanned for future intake.

Project source, tests, and existing product documentation remain authoritative
for runtime behavior. The semantic repository is authoritative for maintained
terminology and its revision history.

## Flow

1. Registration canonicalizes an exact folder, verifies its root
   `AGENTS.md` marker, and captures the current watermark from one explicitly
   configured Annals decisions library. It intentionally does not import
   earlier accounts. A schema-one database first requires the controlled
   `project activate-annals` cutover so all existing non-retired projects share
   one activation watermark.
2. A serial one-shot worker first resumes an exact legacy or new in-flight
   Nucleus correlation, then freezes the current Annals watermark and reads
   bounded immutable pages after each project's separate Annals scan cursor.
3. Conversations resolves the account's single user-authority thread to its
   exact working directory. The deepest current non-retired registered root
   containing it owns the account. A known cwd outside every root is ignored;
   missing or failed cwd lookup remains visible as unassigned intake. The cwd
   is used only during that routing call and is never copied into intake state
   or output; Semantics retains only the selected project and a fixed routing
   outcome.
4. Every valid accepted account is immediately reconciliation-eligible. The
   new path has no confidence, review, disposition, supersession, or
   current-force gate. Preserved legacy admissions, reviews, and their states
   are neither promoted nor reinterpreted.
5. Semantics persists one stable Nucleus job correlation, supplies only the
   normalized statement/context/action/result and occurrence projection plus
   the complete selected repository snapshot, and exposes one successor
   immutable managed tool. Anchors, cursor, project assignment, and the fixed
   routing outcome remain in Semantics SQLite.
6. The tool callback validates the base revision, sequential concept IDs,
   effect invariants, exact Annals library/event/account grounding, active
   project state, and replay safety before atomically appending a revision and
   receipt.

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
resumes one processing item first, scans bounded accepted-account pages, and applies
at most one reconciliation. Pausing a project prevents a late proposal from
committing.

A release-independent, current-user-owned, mode-`0600`, non-hard-linked
maintenance marker prevents any release-pinned runner from entering domain
work during deployment, uninstall, or fail-closed recovery. Lifecycle tooling
never truncates an existing marker and refuses any other shape. A successful
deployment may retain an authenticated marker/receipt pair for an outer
cutover; a later successful same-release invocation releases only that pair.
An unrelated unreceipted marker is preserved. Uninstall and an unprovable
rollback retain the gate. Existing product log files must likewise
be current-user-owned regular non-hard-linked files; deployment makes their
mode `0600` without truncating content before definition registration.

Clockwork records only schedule, definition, binding, and process outcomes. It
does not inspect Semantics domain state or ingest product log bodies. A zero
process exit reports only that the one-shot invocation returned successfully;
the Semantics database and worker report remain authoritative for intake and
commit outcomes.

Service stdout contains only counters and opaque identifiers; stderr contains
only bounded product-owned failure codes and messages. Raw dependency errors,
account statements, context, actions, results, project content, conversation
text, anchors, paths, diffs, commands, tool output, credentials, and Nucleus
prompts must not enter service logs.

## Failure boundaries

Annals cursors advance in the same SQLite transaction that durably records an
account or determines it irrelevant. Each page is fixed to one watermark;
empty pages cannot advance and changed immutable identities fail closed.
Repository revisions and typed effects are append-only. New failed or
unassigned intake and all legacy states stay explicit. Separate legacy and
account mailbox receipts make repeated delivery idempotent and reject
conflicting replay. Operator retry refuses a prior Nucleus job that is active
or whose admitted state cannot be established.

# Data model

Semantics uses SQLite schema version 1. The database is the sole writable
authority; registered project folders contain only their participation marker
and ordinary project-owned material.

## Project registry

`projects` stores the stable ID, lifecycle status, canonical current path,
activation cursor, scan cursor, next sequential concept number, and timestamps.
`project_paths` is append-only path history with exactly one open path per
non-retired project. A move closes the old path and opens the new one without
changing identity or cursor history.

## Repository

`semantic_revisions` has a contiguous per-project revision number, summary,
optional source event ID, and commit time. `semantic_effects` orders typed JSON
effects within the revision. HEAD is obtained by replay; schema 1 deliberately
has no mutable concept projection.

Effects are:

- `define`: create the next stable `cNNNNNN` concept.
- `revise`: change an active concept's canonical label or meaning.
- `differentiate`: record its durable distinction from another active concept.
- `retire`: close a concept, optionally naming an active replacement.
- `reopen`: make a retired concept active again.
- `ground`: cite an exact Decisions event/decision pair or a hashed seed source.
- `unground`: append the withdrawal of a prior decision grounding while
  retaining both its original and withdrawal provenance.

Active canonical labels are unique after trimming, collapsing whitespace, and
Unicode lowercase conversion.
Revision application is all-or-nothing: effects are first replayed against a
candidate copy, and the database transaction commits only a valid complete
revision.

## Intake and reconciliation

`intake_events` durably copies each normalized lifecycle envelope selected for
the project, its opaque source cursor, exact routing metadata, state, attempts,
failure, and optional applied revision. It stores no transcript, diff, command,
tool output, or raw project content. `intake_assignments` appends manual routing
history. A review first reuses the existing non-retired project binding for its
decision ID; its cwd is deliberately null because historical cwd is no longer
a routing fact. First or otherwise unbound events use exact current cwd and the
deepest current registered root.

`request_correlations` stores exactly one requester/job identity, immutable
request bytes and digest, admission state, and mailbox cursor for an attempt.
`mailbox_receipts` caches each tool call's argument digest and exact result.
Identical redelivery is idempotent; conflicting redelivery is rejected. A
successful callback stores the receipt and semantic revision in one domain
transaction before the result is acknowledged to Nucleus.

## Backup and migration

The deployment boundary stops the worker, suspends the public command, proves
the exact worker flock is exclusively held, proves the database is not open,
and privately copies the database plus any `-wal`, `-shm`, or `-journal`
sidecars before candidate doctor can initialize or migrate it. A failed
deployment restores those bytes and all public selectors before restarting the
prior owned service. A rollback that cannot prove service/database quiescence
fails closed before releasing the worker flock: current/public selectors and
the installed plist are removed, so a still-loaded job has no executable
current runner, while the prior plist, selector record, releases, and database
backup are retained. Schema changes must preserve this boundary and add
explicit migration and rollback tests.

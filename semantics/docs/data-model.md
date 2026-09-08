# Data model

Semantics uses SQLite schema version 3. Its database is the sole writable
authority. Registered folders contain their participation marker and ordinary
project-owned material.

## Project registry

`projects` retains the stable ID, lifecycle status, canonical current path,
legacy Decisions activation and scan cursors, next sequential concept number,
and timestamps. `annals_feed_identity` binds the database to one persistent
decisions-library ID. `annals_project_cursors` stores a distinct activation and
scan cursor for each participating project; Annals and legacy cursor bytes are
never interchanged. `project_paths` is append-only path history with exactly
one open path per non-retired project and records both activation histories.
A move closes the old path and opens the new one without changing identity or
either cursor history.

## Repository

`semantic_revisions` has a contiguous per-project revision number, summary,
optional source event ID, and commit time. `semantic_effects` orders typed JSON
effects within the revision. HEAD is obtained by replay; schema 3
has no mutable concept projection.

Effects are:

- `define`: create the next stable `cNNNNNN` concept.
- `revise`: change an active concept's canonical label or meaning.
- `differentiate`: record its durable distinction from another active concept.
- `retire`: close a concept, optionally naming an active replacement.
- `reopen`: make a retired concept active again.
- `ground`: cite an exact Annals library/event/document triple, a historical
  Annals library/event/account triple, a legacy
  Decisions event/decision pair, or a hashed seed source.
- `unground`: append the withdrawal of a prior decision grounding while
  retaining both its original and withdrawal provenance.

Active canonical labels are unique after trimming, collapsing whitespace, and
Unicode lowercase conversion.
Semantics first replays effects against a candidate copy. The transaction
commits only if the whole revision is valid.

## Intake and reconciliation

`account_intake_events` retains complete accepted documents alongside preserved
historical account projections. A new document row contains a local intake ID,
the original Annals library/event/document identities, source filename, digest,
acceptance time, exact text, project, cursor, state, and attempts. It requires no
source anchor. Origin columns are nullable. Each project receives its own intake
with uniqueness on `(library_id, account_id, project_id)`; the historical column
`account_id` holds the producer's document key for new rows.

Insertion and the scanner project's cursor advance are atomic. Projects decide
relevance through their reconciliation agents. A completed empty-effect result
records `no_change_revision` at the examined HEAD, leaves `applied_revision`
absent, and ends as `ignored`; it creates no semantic revision. These facts and
the exact tool receipt commit before acknowledgement. Historical assignments and
source projections keep their original interpretation. A previously retained
account event is not duplicated when its original bytes appear in the new feed.

The schema-one `intake_events`, `intake_assignments`, lifecycle envelope bytes,
statuses, Decisions cursors, and review behavior remain intact for historical
inspection and exact recovery of a legacy in-flight job. They are no longer a
future scan source.

`account_request_correlations` stores exactly one successor requester/job
identity, immutable request bytes and digest, admission state, and mailbox
cursor for an account attempt. `account_mailbox_receipts` caches each successor
tool call's argument digest and exact result. The legacy tables retain their
old bytes and schema/tool identities.
Identical redelivery is idempotent; conflicting redelivery is rejected. A
successful callback stores the receipt and semantic revision in one domain
transaction before the result is acknowledged to Nucleus.

## Backup and migration

The deployment boundary registers the candidate Clockwork definition without
selecting it, disables the prior binding and any owned legacy LaunchAgent,
suspends the public command, verifies exclusive ownership of the exact worker
flock, verifies that the database is closed, and privately copies the database
plus any `-wal`, `-shm`, or `-journal`
sidecars before candidate doctor can initialize or migrate it. A failed
deployment restores those bytes and all public selectors before restarting the
prior scheduler state. Rollback restores the exact prior immutable definition
only when its binding was enabled, or the prior owned legacy LaunchAgent, never
both. A previously absent or disabled binding stays disabled without transient
activation. A prior non-null disabled selection is restored exactly; only a
previously absent or disabled-null binding may retain the candidate digest in
its inactive tombstone because Clockwork has no clear-selection operation. A
rollback that cannot prove scheduler/database quiescence fails closed before releasing
the worker flock: the release-independent maintenance gate remains, scheduler
cleanup is attempted, public selectors are removed, and the prior schedule
record, releases, and database backup are retained. If a newly selected
candidate cannot be cleared back to a prior null selection, its exact private
`current` release selector is retained as ownership evidence; other
unprovable paths remove it.
Semantics retains the exact release bytes referenced by every registered
immutable definition; pruning is a separate explicit lifecycle operation.
Migration 1-to-2 only adds the Annals feed identity/cursor, account intake,
assignment, correlation, and mailbox tables plus the nullable Annals path
activation field. It does not populate Annals cursors or reinterpret legacy
rows. Unreleased schema-two working databases that used an account `cwd`
column are normalized at open: a fixed routing outcome is derived, free-form
new-path failures are bounded, and the cwd column is removed without changing
the schema version. Controlled cutover captures one Annals watermark after
legacy draining; new projects capture their own current watermark. Schema
changes must preserve this boundary and add explicit migration and rollback
tests.

Migration 2-to-3 preserves historical intake, correlations, receipts, cursor
positions, and semantic revisions. It makes origin columns optional, permits
one document intake per project, and adds the no-change result revision. New
`annals_document` grounding has a distinct serialized kind. Old account and
Decisions grounding kinds are unchanged. Immutable document tool schemas have
new IDs; old admitted requests continue to use their original schemas.

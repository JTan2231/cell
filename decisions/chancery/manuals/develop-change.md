# Develop Decisions

Read the repository instructions and the Nucleus operator manual before
changing requester integration, persistent state, deployment, or service
lifecycle. Keep source inclusion based on item or turn event time. Preserve the
host/item/exact-span identity across fork provenance changes and never derive
candidate IDs from model prose, confidence, disposition, observation identity,
or effect metadata.

Keep the `Stop` hook synchronous, content-free, and bounded to durable
session/turn ingestion. Resolve that hint through Conversations rather than
assuming it is a thread ID. A successfully completed nonempty `fileChange`
selects an enacted turn but never supplies decision authority, paths, diffs,
commands, or tool output. The level-0 classifier scope contains all eligible
current-turn user authorities, the immediately preceding assistant proposal
needed to interpret them when one exists, and at most the final assistant
result—not the whole turn or thread. Allow only one validated expansion to
prior normalized context and keep processing serial.

Keep source-resolution deferral fair without adding parallelism. Both regular
and projection selection resume an existing processing row first, skip queued
rows whose retry time has not arrived, and order ready queued work by
retry-ready time so a deferred source yields to other ready observations.

Unavailable-source abandonment is an explicit audited exception, not a queue
cleanup primitive. `observe abandon OBSERVATION_ID --source-unavailable` may
close only a previously observer-deferred pending level-0 correlation with a
retry time, no not-completed marker, and no bound source, job, authority,
verdict, or candidate. It records `complete` / `not_eligible` with the fixed
`conversation_source_abandoned` marker, stores no caller-provided reason,
creates no decision or lifecycle state, and preserves the observer baseline.
The exact repeat is idempotent recovery from uncertain command completion.
Refuse merely unfinished or changed rows, and fail completed-root reconciliation
closed if an abandoned correlation later appears.

The observer baseline is a write-once deployment cutover. Migration may open
the candidate database only after both services are quiesced and its SQLite
files are backed up; switch the `decisions/observer` Clockwork binding only
after activation. A Clockwork definition must name the exact content-addressed
release, explicit interpreter and release-owned runner with their digests,
literal argv, scrubbed non-secret environment, working directory, schedule,
overlap policy, and product-owned log paths. Render the concrete definition
outside the release only after its release ID is known; never register a
template, mutable `current` selector, public frontend, shell command string, or
secret. Tests must prove no pre-baseline reconciliation, foreign-hook
preservation, scrubbed key-free runners, no legacy/Clockwork dual schedule, and
definition-show ownership proof before binding mutation, exact restoration of
previously disabled selectors, maintenance gating before definition
registration and through both switches and commit, retained gating after
unprovable rollback, complete rendered legacy
plist ownership including UID and mode, rollback of both binding states, exact
legacy service state, hook bytes, selectors, and migrated database files.

Tests for observation scheduling or recovery must prove ready-time yielding for
both selectors, processing-row resumption, the complete abandonment guard,
idempotent exact repetition, unchanged baseline and decision/lifecycle tables,
and the later-reconciliation conflict.

Registered schemas and toolsets are immutable. Publish a new schema ID or
toolset version for changed meaning and keep legacy decoders. Keep Nucleus
permissions at none/false/false unless a separately justified contract change
is made. Finish with `decisions/ci.sh` green.

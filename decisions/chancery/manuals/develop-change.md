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

The observer baseline is a write-once deployment cutover. Migration may open
the candidate database only after both services are quiesced and its SQLite
files are backed up; bootstrap the observer only after activation. Tests must
prove no pre-baseline reconciliation, foreign-hook preservation, scrubbed
key-free runners, and rollback of both service states, hook bytes, selectors,
and migrated database files.

Registered schemas and toolsets are immutable. Publish a new schema ID or
toolset version for changed meaning and keep legacy decoders. Keep Nucleus
permissions at none/false/false unless a separately justified contract change
is made. Finish with `decisions/ci.sh` green within 60 seconds.

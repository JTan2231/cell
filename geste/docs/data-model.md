# Data model

Geste uses SQLite schema version 1. The selected database is the only writable
domain authority. It stores stable episode identities and complete immutable
revision snapshots; it stores no mutable HEAD row or materialized search,
report, or graph projection.

Public episode IDs are `eN`. Revision numbers begin at 1 and remain contiguous
within an episode. Creation and revision use immediate, foreign-key-checked
transactions. A revision append verifies the caller's supplied base and writes
nothing on a stale base. A `revision_seals(episode_id, revision, sealed_at)` row
is written last in that transaction. Once present, sealing triggers refuse
later child inserts; update and delete triggers refuse every history-table
mutation. A committed revision without its seal is invalid and cannot be read.

Each revision retains its creation time, exact submitted-byte SHA-256, and all
ordered authored fields, settlement statuses, tags, gaps, source anchors,
support targets, and frozen related-episode links needed to reproduce its
views. The exact input file and upstream source bodies are not retained.

Source anchors are namespaced by source system and kind and retain a stable
reference, optional source revision, optional SHA-256, observation time, role,
label, and explicit supported claims. A source lacking both revision and digest
is locator-only and produces a coverage warning. A verified settlement must
have exact Decisions lifecycle authority support; an unverified settlement
must identify one stored gap.

Revision meaning remains source-owned. In particular, an ordinary Git tag
name is a mutable locator, not an immutable revision. A Git anchor freezes the
basis with a full commit, tree, or tag-object ID, or pairs the tag name with
its peeled object ID or a source digest.

The complete graph is derived from one revision. Claim nodes are visibly
episode-authored; source nodes are locators, not copied facts. Related episode
nodes identify exact historical revisions. The graph includes the source
boundary, structured source revision/digest/observation attributes, explicit
unverified-settlement gap links, and only one node for each exact related
revision even when several relation edges point to it.

SQLite uses foreign keys, WAL for writes, and a bounded busy timeout. The
database and sidecars are private state. Version 0.1 has no migration. A future
schema change must add an old-state fixture, explicit migration command,
quiescent database-plus-sidecar backup, and database-aware rollback before a
new binary is installed.

Schema readiness requires the complete version-one application table, explicit
index, immutability-trigger, and sealing-trigger set. A marker and
`user_version` alone are insufficient.

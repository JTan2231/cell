# Change Geste

Read Geste's product instructions and the exact architecture, CLI, data-model,
installation, and Chancery documents for the changed behavior. Keep manual
v0.1 small: a strict local capture transaction, immutable revisions,
deterministic lexical retrieval, and read-time report and graph projections.

The authority boundary is non-negotiable. Geste owns the episode boundary,
authored process interpretation, source-anchor links, validation, identity,
revision ordering, retrieval rules, and projections. A cited product owns the
referenced record and its current truth. Do not copy upstream bodies, resolve a
manual anchor implicitly, turn assistant prose into enacted authority, or let
an episode lesson become current policy.

Preserve the fail-closed rules when changing capture or storage:

- create and revise are complete immutable snapshots;
- revise atomically compares the caller's explicit base with current HEAD;
- verified settlements require a supporting Decisions lifecycle authority
  source, while unverified settlements require an explicit gap;
- source observation cannot follow the episode basis cutoff;
- related episodes freeze an exact revision;
- validation errors commit no partial identity or revision.

Search examines only current heads at read time and remains fully
deterministic: NFKC/lowercase/collapsed terms, all-term matching, fixed field
weights, and numeric-ID tie-breaking. Report and graph remain projections, not
stored second truths. The graph is bounded to the selected revision and keeps
episode-authored and source-backed nodes visibly distinct.

Use synthetic isolated databases and request fixtures. Exercise strict JSON
and resource bounds, transaction rollback, stale writes, historical reads,
latest-head search including outcome status, stable error codes, partial-schema
and unsealed-state refusal, sealed child-insert refusal, report and exact graph
provenance, bounded regular-file/stdin input, and umask-independent private file
placement as affected. Packaging tests must
prove candidate/provider version matching, content identity, owned selector
validation, idempotent redeploy, update, tamper refusal, and rollback without
touching a domain database.

Any schema change needs an explicit migration, a quiescent database-plus-
sidecars backup boundary, an old-state fixture, and database-aware rollback.
Do not add implicit migration to ordinary commands or the deployer.

Finish with `geste/ci.sh` green. `release.sh` creates a
commit, tag, and remote push; the macOS deployer changes installed state and
`geste init` creates user domain state. None of those effects follows from the
development operation without separate authority.

Continuous ingestion, source-specific runtime adapters, model inference,
embeddings, and network activity are outside manual v0.1. Stop for a new
contract review before assigning their identity, cutoff, privacy, failure, and
recovery behavior.

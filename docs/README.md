# Annals documentation

These documents describe the implemented Annals contracts.

## Decided model

1. A library is one SQLite database containing a forest of rooted trees.
2. Every node has one canonical string, an optional parent, and ordered
   children. There are no node kinds.
3. Ingestion accepts one raw UTF-8 stream with no required record structure.
4. The raw-window adapter cuts the stream into deterministic, non-overlapping
   windows of at most 8,192 bytes. Window boundaries are transport boundaries,
   not conceptual boundaries.
5. The embedded Codex launcher is fixed to `gpt-5.6-terra` with medium
   reasoning and communicates only through standard I/O.
6. The model chooses the conceptual hierarchy. `node_budget`, `max_depth`, and
   `max_children` are hard maxima, not fill targets.
7. Annals accepts only schema-conforming, structurally valid proposals. It does
   not repair or semantically rescore a proposal.
8. Accepted trees, raw input, byte windows, grounding links, model settings,
   prompt/schema versions, limits, and accepted JSON are committed atomically.
9. Generated trees are immutable through node commands and may be removed only
   as complete trees.
10. Search uses one derived SQLite FTS5 row per node and can be rebuilt from
    canonical rows.

Conceptual correctness remains the model's judgment. Deterministic validation
establishes contract and topology correctness, not semantic truth.

## Documents

- [Architecture](architecture.md) describes ingestion and process boundaries.
- [Data model](data-model.md) describes canonical and derived SQLite state.
- [CLI](cli.md) describes commands and output behavior.
- [Search](search.md) describes the implemented lexical retrieval path.
- [Performance results](performance-results.md) records the performance claims
  that apply to this implementation.

## Terminology

**Library**
: One SQLite database managed by Annals.

**Raw input**
: The unchanged UTF-8 string supplied to one ingestion.

**Input unit**
: A stable ID and contiguous byte range produced by the raw-window adapter.

**Proposal**
: The schema-constrained JSON tree emitted by the model.

**Generation run**
: The retained record tying raw input, adapter, model, prompt, policy, accepted
  proposal, tree, and grounding together.

**Support link**
: An explicit relation from a generated node to an input-unit ID.

**Search unit**
: The derived FTS5 record for one canonical node string and breadcrumb.

# Annals architecture

## Purpose

Annals is a local-first CLI for maintaining and searching a forest of topic trees.
Each internal node is a view of a topic; its children provide progressively more
detailed views. Leaf nodes contain source material.

The design is deliberately small:

- one Rust executable;
- one SQLite database per library;
- SQLite FTS5 for search;
- no embeddings, vector index, server, or background daemon;
- no topic assignment or text generation.

The database is the portable library artifact. Import and export formats are
outside the search core.

## Design boundaries

Annals owns:

- the tree structure and sibling order;
- topic and source text;
- optional provenance for source leaves;
- deterministic conversion of node text into searchable units;
- lexical retrieval, tree-aware result grouping, and result display;
- schema initialization, validation, and index rebuilding.

Annals does not own:

- automatically choosing where material belongs;
- producing summaries or intermediate topic views;
- synchronizing multiple writers over a network;
- watching external source files for changes;
- semantic retrieval.

These boundaries keep the canonical model independent of the search engine.
Changing the index does not change what a topic tree means.

## High-level shape

Keep the implementation as modules in one binary rather than as services:

```text
CLI parsing
    |
application commands
    |-- tree operations
    |-- search/index operations
    `-- validation
    |
SQLite repository
    |-- canonical tables
    `-- FTS5-derived tables
```

The Rust modules are `cli`, `db`, `tree`, `ingest`, `index`, `search`, and `render`.
They are code-organization boundaries rather than separate crates.

All mutations go through application commands. The repository exposes
transaction-scoped operations; it should not let callers make a tree mutation
and forget the corresponding index update.

Ingestion is a strict executor boundary. Its caller supplies complete final
child lists for every changed parent, complete replacement content, and exact
subtree membership for deletions. Annals validates that proposed graph and
performs it without choosing placement, balancing branches, merging nodes, or
promoting children.

## Why SQLite and FTS5

SQLite fits the expected workload: a local library, mostly reads, occasional
interactive edits, and a single authoritative file. It provides transactions,
foreign keys, recursive CTEs for tree traversal, and FTS5 for mature lexical
search. There is no operational service to install or keep running.

FTS5 covers the important embedding-free cases:

- terms and phrases;
- prefix matching;
- BM25 ranking;
- snippets and match highlighting;
- field weighting for titles, breadcrumbs, and body text.

Use a bundled SQLite build with FTS5 enabled so behavior does not depend on the
host's SQLite installation. At connection startup, enable foreign keys, set a
reasonable busy timeout, and use WAL mode. WAL permits readers while a write is
in progress, but Annals should still assume there is only one writer at a time.

SQLite should be reconsidered only if Annals becomes a multi-user network
service with sustained concurrent writes, not merely because the library gets
larger.

## Canonical data and derived data

The `nodes` and `sources` tables are canonical. Everything in `search_units`
and its FTS5 index is derived and may be deleted and rebuilt.

This distinction is important:

- restoring canonical tables restores the user's library;
- an index format can change without changing canonical content;
- an interrupted or suspect index can be repaired with `annals reindex`;
- search code never needs to write back into canonical content.

Node identifiers are stable SQLite integer IDs. A move changes a node's parent
and breadcrumb, not its identity. Human-readable paths are presentation and
search data, not identifiers, because titles may be edited and may repeat.

## Tree representation

Use an adjacency list: each node has an optional `parent_id`; a null parent is a
root. This is enough for the design. Recursive CTEs handle ancestors,
descendants, breadcrumbs, and subtree-scoped search.

"More detailed than the parent" is a semantic authoring rule, not something the
database can infer from arbitrary text. Annals preserves the chosen hierarchy
and ordering but does not pretend to validate that relationship automatically.
It can still validate the structural rules below.

A closure table or materialized path would make some reads cheaper, but it
would also add write and repair complexity. Add one only after measurements
show recursive traversal to be a bottleneck.

The application and database together preserve these invariants:

1. The structure is a forest: every node has at most one parent and there are no
   cycles.
2. A source node cannot have children.
3. Nodes with children are topics.
4. Sibling positions are unique and non-negative.
5. A source node has its corresponding source record.
6. Search-unit byte ranges are within the current node text and do not split a
   UTF-8 code point.

The intended complete-tree condition is that every leaf is a source. It can be
useful to create an empty topic while editing, so the database may temporarily
contain topic leaves. `annals validate` reports them as warnings. Sources having
children, cycles, and dangling references are always errors.

## Searchable units

Search the nodes as a flat collection of units; do not navigate downward by
selecting one apparently relevant branch at each level. A mistaken match near
the root must not hide a relevant source elsewhere in the forest. The tree is
used for scoping, context, and result organization after retrieval.

Each short topic view normally produces one search unit containing its own
text. Long topic views and source leaves may produce multiple passage units.
Chunking is an indexing concern: it must never add conceptual nodes to the
user's tree.

Each unit records:

- its owning node and deterministic sequence number;
- a snapshot of the node title and breadcrumb;
- its text;
- UTF-8 byte offsets into the node text when it is a passage;
- a content hash and indexer version.

Chunk on paragraph boundaries where practical, then use a bounded text window
for oversized paragraphs. A small overlap can preserve phrases crossing a
boundary. Exact sizes should be configuration constants backed by relevance
and latency tests, not user-facing schema. The same input and indexer version
must always produce the same units.

Do not concatenate or index an entire subtree as the parent's text. That would
duplicate terms at every level and make large, broad branches win unrelated
queries.

## Retrieval pipeline

For a normal query:

1. Parse structural filters such as `--within`, `--kind`, and `--detail`.
2. Resolve exact node IDs and exact titles before full-text retrieval.
3. Convert ordinary user text to a safe FTS expression. Do not pass arbitrary
   punctuation through as raw FTS query syntax.
4. Retrieve an intentionally wider FTS5 candidate set using BM25.
5. Join candidates to their nodes and apply hard scope and kind filters.
6. Collapse several matching passages from one node to its best passage while
   retaining secondary snippets as supporting matches.
7. Group or suppress redundant ancestor/descendant hits and diversify across
   branches.
8. Render a title, breadcrumb, node kind, highlighted snippet, and match reason.

Use BM25 field weights so the title matters most, followed by breadcrumb and
body. Exact title matches are inserted ahead of, or given a clear boost over,
ordinary FTS matches. Keep the precise weights in one place and test them; they
are ranking policy, not data model.

Tree-aware ranking should stay conservative:

- a node's own text match is a direct match;
- a matching descendant may make an ancestor useful context, but not a direct
  textual hit;
- if descendant relevance is propagated upward, use the strongest descendant
  with distance decay rather than summing all descendants;
- cap results per branch so one large branch cannot fill the screen;
- prefer the requested level of detail instead of globally preferring shallow
  or deep nodes.

Detail modes are simple presentation/reranking policies:

- `overview`: lean toward topic nodes;
- `balanced`: default to the best direct match and collapse nearby relatives;
- `source`: lean toward source leaves and passage matches.

Typos are a separate concern from semantic search. Start without special typo
machinery. If query logs or a relevance suite show a need, add trigram matching
over short titles only; indexing every source body as trigrams is unlikely to
justify its space cost.

## Search data flow

```text
query
  -> exact title/ID lookup + FTS5 retrieval
  -> scoped candidate rows
  -> per-node passage collapse
  -> ancestor/descendant collapse and branch diversification
  -> breadcrumbs and snippets
  -> terminal output
```

A subtree scope is computed with a recursive CTE beginning at the selected
node, then joined to FTS candidates by `node_id`. Apply the scope in SQL rather
than fetching a global top-N and filtering afterward; post-filtering could omit
the best results within a small subtree.

Search is read-only. A search command should use one SQLite snapshot so node
text, breadcrumb data, and results are internally consistent even if another
process commits an edit.

## Write data flow and transactions

Every user-visible mutation runs in one transaction, normally `BEGIN
IMMEDIATE` so write contention is discovered before work begins:

1. Validate the requested operation against the current tree.
2. Change canonical rows.
3. Determine which nodes' search units are affected.
4. Regenerate those units; FTS triggers mirror the changes.
5. Run cheap postcondition checks.
6. Increment the canonical library revision once.
7. Commit.

If indexing fails, roll back the content change too. This keeps ordinary edits
immediately searchable and avoids introducing an index job queue. Full
reindexing remains available for repair and version changes.

Specific mutations have the following impact:

- Editing a node's title or text regenerates its units. A title edit also
  changes descendant breadcrumbs, so descendant units are refreshed.
- Moving a subtree first verifies that the new parent is not a source or a
  member of the moving subtree. It then updates the root's parent and sibling
  order and regenerates breadcrumb-bearing units for the whole subtree.
- Reordering siblings only changes positions and needs no text reindex.
- Deleting a node deletes its entire subtree with foreign-key cascades. This is
  an explicit, confirmed operation; children are never silently promoted.

Annals does not use soft deletion. Backups and explicit confirmation provide a
clear model for a local CLI. Before a destructive subtree deletion, show the
resolved node, path, and descendant count.

Sibling ordering uses spaced integer positions. Ingestion treats those values
as a mechanical encoding of supplied child arrays and never rewrites a parent
that the document did not name.

## Reindexing and recovery

`annals reindex` is deterministic and rebuilds only derived state:

1. Acquire the write transaction.
2. Clear `search_units`.
3. Traverse canonical nodes in stable order and regenerate units.
4. ask FTS5 to rebuild its external-content index as a consistency repair;
5. store the current indexer version;
6. commit.

The command should build all regenerated data in the transaction. A failure
leaves the prior working index in place through rollback. Reindexing directly
regenerates the derived tables without a shadow-table swap.

At startup, compare the stored indexer version to the executable's version. If
they differ, return a useful instruction to run `annals reindex`; do not silently
serve a partially stale index. Content hashes allow `annals validate` to detect
individual stale units.

Useful validation checks include foreign-key integrity, absence of cycles,
source nodes without children, source metadata presence, sibling-position
uniqueness, stored unit hashes, and FTS5 integrity.

## Schema initialization

`schema.sql` is the current data model. `annals init` executes it once while
creating a new library. Existing libraries are opened directly. Change the
schema definition itself when the data model changes.

Changes limited to `search_units` or FTS configuration should prefer rebuilding
derived state rather than rewriting canonical rows.

## Practical quality bar

Before tuning ranking, maintain a small checked-in relevance suite covering:

- exact titles and phrases;
- rare words, acronyms, and identifiers;
- a result below an initially unrelated-looking root;
- broad queries that should return a topic view;
- precise queries that should return a source passage;
- several matching chunks from one source;
- overlapping ancestor and descendant matches;
- subtree and kind filters.

Record expected nodes and acceptable ordering groups. This makes changes to
tokenization, chunking, weights, and result collapsing deliberate rather than
subjective.

# Implementation plan

## Approach

Build one synchronous Rust binary around one SQLite database. Keep canonical
tree data and derived full-text data in that database. Add no service process,
ORM, async runtime, embedding model, or second index.

The first useful release should prove three things:

1. tree invariants remain correct through ordinary edits;
2. SQLite FTS5 gives useful lexical retrieval over nodes and long-body passages;
3. tree-aware result grouping improves navigation without hiding direct
   matches.

## Minimal Rust stack

Runtime dependencies should initially be limited to:

- `clap` with derive support for command parsing;
- `rusqlite`, linked to a known SQLite build with FTS5 enabled;
- `serde` and `serde_json` for the versioned JSON output contract; and
- `thiserror` for a small typed application error enum.

Three focused correctness dependencies are reasonable: `unicode-normalization`
for shared NFKC search keys, `sha2` for versioned content hashes, and `time` for
UTC RFC 3339 timestamps. Avoid a UUID dependency: SQLite `INTEGER PRIMARY KEY`
assigns stable node IDs within a library.

Use the standard library for paths, file and standard-input handling, timing,
and text plumbing. Never derive identity from a title, breadcrumb, or parent;
those values may change while the integer node ID remains stable.

Use raw, reviewed SQL. Migration files live in the repository and are embedded
with `include_str!`; a small migration runner applies them in order inside
transactions and records the schema version. Do not introduce an ORM or a
migration framework for the initial schema.

Development-only dependencies may include a temporary-directory helper and a
CLI assertion helper. Add a benchmark harness only when the representative
corpus fixture exists.

## Proposed module boundaries

Keep the crate as a single binary until another consumer actually needs a
library API:

```text
src/
  main.rs          parse arguments, call the application, choose exit code
  cli.rs           clap command and option definitions
  app.rs           command orchestration and transaction boundaries
  db.rs            connection setup, pragmas, backup, integrity checks
  migrations.rs    embedded raw migrations and schema-version checks
  model.rs         Node, NodeKind, Source, SearchResult, and ID types
  tree.rs          create/edit/move/delete operations and invariants
  index.rs         deterministic search-unit construction and FTS updates
  search.rs        query normalization, retrieval, ranking, and grouping
  render.rs        human rendering and versioned JSON envelopes
  error.rs         typed errors, JSON codes, and numeric exit-code mapping
migrations/
  0001_initial.sql
tests/
  fixtures/
```

These are responsibility boundaries, not independent architecture layers.
Merge files that remain tiny. SQL specific to one operation may stay beside
that operation; shared schema and migration SQL should not.

## Database setup

On connection:

- enable foreign keys;
- set a finite busy timeout;
- use WAL for an initialized writable library;
- verify the supported schema version; and
- verify FTS5 during `init` with a small feature probe.

Canonical `nodes` and `sources` tables contain the forest, ordering, title,
body, and provenance needed to recreate search units. A node with no parent is
a root; there is no separate trees table. FTS and search-unit rows are derived.
Normal edits update canonical and derived rows in the same transaction.
`reindex` discards and recreates only the derived rows.

Start with an adjacency list and recursive CTEs. Add neither a closure table nor
cached materialized paths until a benchmark demonstrates that ancestor or
subtree traversal is a bottleneck.

## Milestones

### 1. Executable, migrations, and library lifecycle

- Create the crate and global CLI options.
- Implement connection setup and the first raw migration.
- Implement `init`, schema-version checks, and FTS5 detection.
- Define typed errors, exit codes, and the JSON success/error envelope.
- Add integration tests for creating, reopening, and refusing to overwrite a
  library.

Done when a fresh database can be created and reopened, an unsupported newer
schema is rejected without writes, and all failures have deterministic exit
codes.

### 2. Tree maintenance

- Implement tree create/list/show/delete.
- Implement node add/show/children/edit/move/delete.
- Enforce parent existence, acyclicity, root rules, source-as-leaf, and sibling
  ordering inside transactions.
- Implement confirmation behavior for recursive deletion.
- Add `validate`, including SQLite and application-level invariant checks and
  incomplete topic-leaf warnings.

Done when the CLI can construct and rearrange multiple trees, and every tested
invalid edit leaves the database logically unchanged.

### 3. Lexical indexing and basic search

- Define deterministic search-unit boundaries. Short node bodies produce one
  unit; any long topic or source body produces bounded, overlapping passages
  without changing the visible tree.
- Create and maintain the external-content FTS5 index.
- Normalize plain queries and construct safe exact title/path lookup plus FTS
  phrase and token retrieval: AND first, then a controlled OR fallback.
- Add final-token title-prefix fallback only after exact and ordinary lexical
  passes; do not add typo matching initially.
- Implement global, subtree-scoped, and kind-filtered search. A root subtree is
  exactly one tree.
- Return breadcrumbs and the best matching passage for each node.
- Implement `reindex` and detect stale or inconsistent derived rows in
  `validate`.

Done when edits are immediately searchable, a complete reindex gives the same
ranked candidates on unchanged content, and user punctuation cannot become
unintended FTS syntax.

### 4. Tree-aware result presentation

- Collapse redundant ancestor/descendant runs without discarding their direct
  match evidence.
- Propagate only bounded, distance-decayed supporting relevance upward; do not
  sum an unlimited number of descendant scores.
- Diversify the first result page across branches.
- Implement `overview`, `balanced`, and `source` detail preferences.
- Add `--explain` output that names exact/phrase/FTS and grouping signals.

Done when repeated hits from one branch are presented as one navigable group,
but an exact direct match remains discoverable and can be explained.

### 5. Operational hardening

- Implement consistent backup, stats, and non-terminal behavior.
- Test interruption and rollback around mutations and reindexing.
- Exercise concurrent readers plus one writer under WAL.
- Run the relevance suite and performance corpus described below.
- Document the measured limits rather than adding infrastructure preemptively.

Done when recovery instructions are tested, benchmark results are recorded,
and all version-one acceptance criteria pass.

## Test strategy

### Unit tests

Test pure behavior without SQLite where possible:

- query normalization and query-expression construction;
- passage boundaries and byte/character offsets;
- compact breadcrumb excerpts and snippet selection;
- rank grouping, distance decay, and branch quotas;
- error-to-exit-code and error-to-JSON-code mappings; and
- sibling position calculations.

### Database integration tests

Each test gets a fresh temporary library and uses the same migrations and
connection setup as the binary. Cover:

- migration order, repeat opening, and newer-schema refusal;
- foreign-key enforcement and transaction rollback;
- adding, moving, reordering, and deleting nodes;
- cycle attempts, root mutation attempts, and source-child rejection;
- recursive CTEs for ancestors, descendants, and breadcrumbs;
- FTS maintenance after create, edit, move, and delete;
- deterministic `reindex` results;
- corruption or stale-index detection by `validate`; and
- readers operating while one writer commits under WAL.

### CLI integration tests

Invoke the compiled binary for a small set of contract-level cases:

- human and JSON output stay on the correct output streams;
- JSON is one valid document with `format_version: 1`;
- no search matches exits zero with an empty array;
- missing targets, invalid input, conflicts, and SQLite failures map to the
  documented numeric codes;
- recursive deletion refuses non-interactive execution without `--yes`; and
- `--body-file -` handles UTF-8 input and rejects invalid combinations.

Do not snapshot every line of human prose. Assert important identifiers and
structure so wording can improve without rewriting the suite.

## Relevance evaluation

Code correctness tests cannot establish whether ranking is useful. Maintain a
small, reviewable fixture library and a JSONL judgment file. Each judgment
contains:

```json
{
  "query": "write skew",
  "scope_node_id": null,
  "kind": "all",
  "relevant_node_ids": [42],
  "preferred_primary_id": 42,
  "case": "technical term"
}
```

Include at least these query classes:

- exact topic text;
- quoted or distinctive source phrase;
- multiple terms in a different order;
- broad topic query with several relevant branches;
- narrow query whose best answer is a source passage;
- acronym, identifier, and punctuation-heavy technical text;
- prefix or incomplete final token;
- subtree-scoped query; and
- an ancestor and descendant that both directly match.

Track `Recall@10`, reciprocal rank of the first judged result, and whether the
preferred branch representative appears in the first page. Keep the individual
failed queries visible; a single aggregate score is not sufficient for a small
judgment set.

Before changing ranking weights, record the current results and the proposed
results against the same judgments. Version one is acceptable when all exact
topic and distinctive-phrase cases are found in the first three results,
`Recall@10` is at least 0.90 across the fixture, and tree grouping introduces no
regression in scoped-query recall. These thresholds are project gates, not a
claim about arbitrary corpora.

## Performance benchmarks

Generate deterministic fixture libraries at 1,000, 10,000, and 100,000 search
units. Include shallow and deep trees, many siblings, long chunked topic and
source bodies, and queries that match common as well as rare terms. Record
corpus byte size, SQLite version, hardware, and whether the filesystem cache is
warm.

Measure:

- global and subtree search latency at p50 and p95;
- breadcrumb/ancestor lookup for shallow and deep nodes;
- single-node add, edit, move, and delete latency;
- bulk import transaction throughput, even if import is initially only a test
  helper;
- full `reindex` time and resulting database size; and
- reader latency while one writer commits.

Provisional local-laptop gates for 100,000 units are warm-search p95 below
150 ms for a 10-result page, ordinary single-node mutation p95 below 50 ms, and
a full reindex below 60 seconds. Run each operation enough times to make p95
meaningful and report the raw command and fixture seed. If a gate fails,
profile the SQL and indexes first; do not immediately replace SQLite.

## Version-one acceptance criteria

Version one is ready when:

- all documented commands and JSON/exit-code contracts are implemented;
- migrations create a reproducible schema on a fresh library;
- tree invariants survive the full integration test matrix;
- every canonical edit is reflected in search in the same transaction;
- `reindex` can reconstruct all derived data and preserves search results on
  unchanged content;
- search supports global, subtree (including one tree by its root), and kind
  scopes;
- results include stable node IDs, breadcrumbs, snippets, and explainable match
  signals;
- the checked-in relevance gates pass;
- the 100,000-unit provisional performance gates pass or a measured exception
  is documented; and
- backup and integrity-check recovery steps have been exercised on a copy of a
  fixture library.

## Explicit deferments

Do not add these to the base implementation:

- embeddings, vector storage, approximate nearest-neighbor search, or an
  embedding provider abstraction;
- automatic topic assignment, summarization, or generated text;
- a server, remote database, accounts, synchronization, or collaborative
  editing;
- an ORM, async runtime, background worker, or plugin system;
- Tantivy, Elasticsearch, or any second lexical index;
- a closure table, nested-set model, or materialized path cache;
- machine-learned ranking or query rewriting;
- raw FTS5 query syntax as a public CLI contract;
- arbitrary source fetching, crawling, MIME extraction, or attachment storage;
- cross-tree node moves, merge semantics, or history/undo; and
- shell completion, an interactive TUI, and broad import/export formats.

Revisit a deferment only with a concrete failing relevance case, performance
measurement, or workflow requirement.

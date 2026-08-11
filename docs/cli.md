# CLI design

## Goals

The Annals CLI should make tree edits explicit, make destructive operations
hard to perform accidentally, and keep search useful in both a terminal and a
script. The command surface below is a proposed version-one contract. It is
small enough to implement directly over SQLite and FTS5.

Examples use `annals.db` in the current directory:

```sh
annals init --library ./annals.db
annals --library ./annals.db tree list
```

`--library PATH` is a global option. Resolution order is:

1. `--library PATH`;
2. `ANNALS_LIBRARY`;
3. `./annals.db`.

Commands should print the resolved path in error messages. They should not
silently search parent directories for another database.

Other global options are:

- `--json` for one JSON document on standard output;
- `--quiet` to suppress successful human-oriented mutation output;
- `--no-color` to disable color even when standard output is a terminal;
- `-v` to add diagnostics to standard error. Repeating it may add detail, but
  must never change a result.

## Stable identifiers

Nodes use SQLite integer primary keys. An ID is stable within its library: it
does not change when the node's title, body, sibling position, or parent
changes. A tree has no separate row or ID; its root node ID identifies it.

Editing commands accept exact decimal node IDs. Titles and display paths may
repeat or change, so they are conveniences for browsing and search rather than
identity. Search-unit IDs belong to derived index data and are not accepted by
editing commands.

## Initialization and inspection

```text
annals init [--library PATH]
annals stats
annals validate
annals backup OUTPUT
annals reindex
```

`init` creates a new library, applies all migrations, and verifies that its
SQLite build includes FTS5. It refuses to replace an existing file.

`stats` reports root, node, source, and indexed-unit counts, the schema version,
database size, and whether the full-text index is current.

`validate` runs SQLite integrity and foreign-key checks, then checks Annals
invariants: every non-root node has one existing parent, roots have none, no
cycles exist, source nodes are leaves with source metadata, sibling positions
are valid, and the derived search index agrees with canonical rows. A topic
leaf is reported as an incomplete-tree warning, not structural corruption.
Validation does not repair data.

`backup OUTPUT` uses SQLite's backup API to create a consistent copy. It
refuses to replace `OUTPUT`; a later implementation may add an explicit
`--force` if that proves necessary.

`reindex` recreates search units and FTS rows from canonical node data. It is
safe to run repeatedly. The command reports indexed node and unit counts.

## Tree commands

```text
annals tree create --title TITLE [--body TEXT | --body-file PATH]
annals tree list
annals tree show ROOT_NODE_ID [--depth N]
annals tree delete ROOT_NODE_ID [--yes]
```

`tree create` appends a root topic node to the forest in one transaction. The
title must not be empty after trimming. Its body defaults to empty;
`--body-file -` reads a UTF-8 body from standard input. `--body` and
`--body-file` are mutually exclusive.

`tree list` returns each root node ID, root title, and subtree node count.
`tree show` renders an indented tree. `--depth` limits display depth; it does
not alter the tree.

Deleting a tree deletes all of its nodes and derived index rows. In a terminal,
the command first shows the affected node count and asks for confirmation. In
non-interactive use it fails unless `--yes` is present.

## Node commands

```text
annals node add --parent NODE_ID --kind topic|source \
    --title TITLE [--body TEXT | --body-file PATH]
    [--locator VALUE] [--media-type TYPE] [--checksum VALUE]
    [--captured-at RFC3339]
    [--position N]

annals node show NODE_ID
annals node children NODE_ID
annals node edit NODE_ID [--kind topic|source] \
    [--title TITLE] [--body TEXT | --body-file PATH | --clear-body]
    [--locator VALUE] [--media-type TYPE] [--checksum VALUE]
    [--captured-at RFC3339]
annals node move NODE_ID --parent NEW_PARENT_ID [--position N]
annals node delete NODE_ID [--recursive] [--yes]
```

New children append to the parent's children unless `--position N` is given.
The flag is a zero-based ordinal among the destination's children. Physical
SQLite positions may be spaced integers so insertion normally updates only one
row; that storage detail is not exposed in JSON.

`--locator`, `--media-type`, `--checksum`, and `--captured-at` are provenance
metadata for source nodes. A locator may be a URL, path, citation, or another
opaque value; Annals does not fetch it. Supplying source metadata for a topic
is an input error. The following structural rules are enforced before
committing:

- a source node cannot have children;
- changing a topic to a source is allowed only when it has no children;
- a child cannot be added or moved beneath a source;
- a node cannot be moved beneath itself or one of its descendants;
- root nodes cannot be moved or deleted through `node`; and
- moving a node between trees is not supported in version one.

`node edit` changes only supplied fields. Supplying no change is a usage error.
An empty body is set explicitly with `--clear-body`. `--body-file -` may be
used for piped UTF-8 input; it cannot be combined with an interactive
confirmation that also needs standard input. Changing a topic to a source
creates its provenance row; changing a source to a topic removes that row.

Deleting a leaf needs no extra flag. Deleting a non-leaf requires
`--recursive`; Annals displays the subtree size and asks for confirmation when
attached to a terminal. Non-interactive recursive deletion additionally
requires `--yes`.

## Search

```text
annals search QUERY [--within NODE_ID]
    [--kind all|topic|source]
    [--detail overview|balanced|source]
    [--limit N] [--explain]
```

Defaults are `--kind all`, `--detail balanced`, and `--limit 10`. `--within`
restricts results to a node and its descendants. Supplying a root node ID is
therefore the way to search exactly one tree.

The ordinary query is plain user text, not raw FTS5 syntax. Annals normalizes
it and builds escaped FTS expressions itself. Whitespace separates terms and
double quotes preserve a phrase. Punctuation cannot change the SQL or FTS
expression unexpectedly. An advanced raw-query mode is deliberately absent
from version one.

Detail preference affects grouping, not eligibility:

- `overview` favors a directly matching topic when nearby descendants also
  match;
- `balanced` selects the strongest direct result from an ancestor/descendant
  run and nests supporting matches beneath it; and
- `source` favors directly matching source leaves while still including their
  topic breadcrumb.

Search resolves exact IDs, normalized titles, and normalized paths, then uses
an FTS5/BM25 AND pass with a controlled OR fallback when results are sparse. A
final-token title-prefix fallback may also be used. Typo matching is deferred
until relevance tests establish a need. Tree-aware grouping and branch
diversification follow retrieval; search does not descend through a single
chosen root branch. `--explain` adds match signals and grouping reasons for
debugging, but its diagnostic details are not a stable machine API.

A human result contains, in order:

1. rank, node kind, and full node identifier;
2. a breadcrumb made from ancestor titles;
3. the matching snippet with terms highlighted when color is enabled; and
4. nested related hits, if nearby nodes on the same branch also matched.

No matches is a successful search: human output says `No matches`, JSON has an
empty result array, and the process exits zero.

## Output contracts

Human output is intended to be read, not parsed. Color is enabled only for a
terminal and honors `NO_COLOR`. Data goes to standard output; progress,
warnings, and diagnostics go to standard error.

`--json` emits exactly one UTF-8 JSON object and no decoration. All JSON output
has a top-level format version so fields can evolve deliberately:

```json
{
  "format_version": 1,
  "ok": true,
  "data": {
    "query": "transaction isolation",
    "results": [
      {
        "rank": 1,
        "node_id": 42,
        "kind": "topic",
        "title": "Isolation",
        "breadcrumb": [
          {"node_id": 1, "title": "Databases"},
          {"node_id": 42, "title": "Isolation"}
        ],
        "snippet": "...transaction isolation...",
        "match_reasons": ["phrase", "lexical"],
        "related_hits": []
      }
    ]
  }
}
```

`rank` is stable only within that response. Raw BM25 values are not part of the
public JSON contract because their scale changes with corpus and query shape.
Mutation results return the affected node IDs. List commands return arrays,
even for zero rows.

With `--json`, an expected error is a single object on standard error:

```json
{
  "format_version": 1,
  "ok": false,
  "error": {
    "code": "would_create_cycle",
    "message": "the requested parent is inside the node's subtree"
  }
}
```

Human error wording may improve over time; the short JSON error `code` is the
machine-facing value.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | Success, including a search with no matches. |
| 1 | Unexpected runtime failure. |
| 2 | Invalid command syntax, invalid title/body input, or an unsupported option combination. |
| 3 | Requested library, tree, or node was not found. |
| 4 | The operation conflicts with a tree invariant or needs confirmation. |
| 5 | SQLite, migration, integrity, or index failure. |

The JSON error code provides finer detail. The numeric set stays intentionally
small.

## Safety and concurrency

- Every mutation and its ordinary FTS maintenance occur in one SQLite
  transaction.
- A failed invariant check rolls the whole command back.
- `init` and `backup` do not overwrite files.
- Recursive deletion is explicit and confirmable as described above.
- The CLI uses a finite SQLite busy timeout and reports lock contention rather
  than waiting forever.
- Opening a database with a newer unsupported schema version fails without
  writing to it.
- Search indexes are derived data. `reindex` may replace them, but it never
  rewrites canonical node titles, bodies, provenance, or hierarchy.
- Search and read commands never trigger an implicit migration. Migrations run
  only when a write-capable command opens an older supported database, and the
  eventual implementation should provide a visible migration notice.

## Examples

```sh
# Start a library and make a root topic.
annals init --library ./annals.db
annals --library ./annals.db tree create \
  --title "Databases" --body "Notes about database systems."

# Add successively more detailed views. IDs shown here are placeholders.
annals --library ./annals.db node add \
  --parent ROOT_ID --kind topic --title "Transactions" \
  --body "Atomicity, consistency, isolation, and durability."
annals --library ./annals.db node add \
  --parent TRANSACTIONS_ID --kind source --title "Isolation paper" \
  --body-file ./paper.txt --locator "https://example.test/paper"

# Search globally, then inside one branch.
annals --library ./annals.db search "transaction isolation"
annals --library ./annals.db search "serializable" \
  --within TRANSACTIONS_ID --kind source --explain

# Consume results from a script.
annals --library ./annals.db --json search "write skew" --limit 5

# Move safely, verify invariants, and rebuild derived search data.
annals --library ./annals.db node move NODE_ID --parent NEW_PARENT_ID
annals --library ./annals.db validate
annals --library ./annals.db reindex
```

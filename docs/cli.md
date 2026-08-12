# CLI contract

## Global options

```text
annals [--library PATH] [--json] [--quiet] [--no-color] [-v...] COMMAND
```

The library path resolves in this order:

1. `--library PATH`;
2. `ANNALS_LIBRARY`;
3. `./annals.db`.

`--json` emits one success object on standard output or one error object on
standard error. `--quiet` suppresses successful human-readable mutation
messages. `--no-color` disables highlighting. `-v` prints the resolved library
path on standard error in human mode.

## Library commands

```text
annals init
annals stats
annals validate
annals backup OUTPUT
annals reindex
```

`init` creates a new SQLite library and refuses to replace an existing path. It
checks that the bundled SQLite build provides FTS5 and creates an empty current
search index.

`stats` reports the revision; root, node, raw-input, generation-run,
support-link, and search-unit counts; file size; and index freshness.

`validate` checks SQLite, forest topology, generation grounding, retained input
digests, exact adapter-produced ranges, and the derived index. It does not
repair data.

`backup` creates a consistent SQLite copy and refuses to replace its output.

`reindex` replaces all derived search rows with one row per canonical node. It
does not change canonical rows or increment the library revision.

## Generated ingestion

```text
annals ingest INPUT
    [--node-budget N]
    [--max-depth N]
    [--max-children N]
```

`INPUT` is a UTF-8 file or `-` for standard input. Empty or non-UTF-8 input is
rejected before model invocation. Defaults are:

```text
--node-budget 32
--max-depth 6
--max-children 6
```

All three values are hard maxima, not targets. The root is depth zero. The
model may return a smaller tree and branches of unequal depth.

Ingestion uses the embedded Codex launcher fixed to `gpt-5.6-terra` with medium
reasoning. An installed and authenticated `codex` executable is required. The
launcher receives one complete prompt on stdin and returns one schema-constrained
JSON proposal on stdout. Model progress is forwarded on stderr.

The accepted input is cut by the `raw-window` version `1` adapter into stable,
non-overlapping windows of at most 8,192 bytes. Those windows are transport
units and do not determine the tree.

After schema and deterministic topology checks pass, raw input, SHA-256 digest,
adapter metadata, model settings, prompt/schema versions, policy, accepted
proposal JSON, windows, nodes, support links, revision, and search rows are
committed together. A model or validation failure writes nothing.

Human output reports the generated root, node count, and unit count. JSON data
has this shape:

```json
{
  "root_node_id": 1,
  "node_ids": [1, 2, 3],
  "input_id": 1,
  "generation_run_id": 1,
  "revision": 1
}
```

Generated trees are immutable through `node` commands. Use `tree delete` to
remove one in full.

## Tree commands

```text
annals tree create --text TEXT
annals tree list
annals tree show ROOT_NODE_ID [--depth N]
annals tree delete ROOT_NODE_ID [--yes]
```

`tree create` adds a manual root. `TEXT` must be nonempty and have no leading or
trailing whitespace.

`tree list` returns each root integer ID, text, and subtree count. `tree show`
uses depth-first order; `--depth` limits display only.

`tree delete` removes the complete tree. In interactive human use it asks for
confirmation. JSON and other non-interactive use require `--yes`. Deleting a
generated tree also removes its generation run, windows, support links, and
unshared retained input.

## Node commands

```text
annals node add --parent NODE_ID --text TEXT [--position N]
annals node show NODE_ID
annals node children NODE_ID
annals node edit NODE_ID --text TEXT
annals node move NODE_ID --parent NEW_PARENT_ID [--position N]
annals node delete NODE_ID [--recursive] [--yes]
```

These mutation commands apply only to manual trees. Each node contains the same
one-string representation; there is no kind flag.

`--position` is a zero-based sibling ordinal. Omitting it appends. A position
past the current sibling list is rejected.

`node move` accepts only non-root nodes, rejects cycles, and keeps a subtree
within its existing root. `node delete` accepts a leaf directly. A node with
descendants requires `--recursive` and confirmation; non-interactive use also
requires `--yes`. Roots must be removed with `tree delete`.

Every successful canonical mutation rebuilds search rows and increments the
library revision in the same transaction.

## Search

```text
annals search QUERY [--within NODE_ID] [--limit N] [--explain]
```

Whitespace separates terms and double quotes preserve a phrase. `--within`
restricts retrieval to one node and its descendants. `--limit` defaults to 10
and accepts 1 through 100. `--explain` includes unstable ranking diagnostics.

Search recognizes exact node IDs, normalized complete breadcrumbs, and
normalized complete node text before lexical FTS5 retrieval. It returns node
text, integer ID, breadcrumb, optional highlighted snippet, and match reasons.

## JSON and exit behavior

Success:

```json
{"ok":true,"data":{}}
```

Failure:

```json
{"ok":false,"error":{"code":"stable_code","message":"description"}}
```

Exit categories are:

| Code | Meaning |
| ---: | --- |
| 0 | Success |
| 1 | Unexpected process, I/O, or JSON failure |
| 2 | Invalid command or input |
| 3 | Missing library, tree, or node |
| 4 | Invariant or confirmation conflict |
| 5 | SQLite, integrity, or index failure |

Terminal rendering escapes control characters from stored text. Color, when
enabled, is used only for search highlights.

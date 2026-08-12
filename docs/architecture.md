# Annals architecture

## Purpose

Annals turns arbitrary UTF-8 text into a grounded conceptual tree and stores it
locally. Every node is homogeneous: it has one string, an optional parent, and
an ordered position among siblings. A root is the shortest standalone,
corpus-relative description the model judges sufficient to identify and
encompass the conceptual family. Each child is a narrower refinement.

There is no objective semantic score in the application. The model judges the
hierarchy; Annals enforces the mechanical contract around that judgment.

The implementation is one Rust executable, one SQLite database per library,
an embedded Codex process bundle, and SQLite FTS5. It has no daemon, server,
embedding index, or generic model-provider layer.

## Ingestion data flow

```text
raw UTF-8 input
  -> deterministic raw-window units
  -> complete prompt on child stdin
  -> pinned Codex subprocess
  -> schema-constrained JSON on child stdout
  -> deterministic acceptance checks
  -> one SQLite transaction for canonical and derived state
```

`annals ingest INPUT` reads a file or `-` for standard input. The input must be
nonempty UTF-8 and is retained unchanged after acceptance.

### Raw-window adapter

The adapter has the recorded name `raw-window` and version `1`. It emits
ordered, non-overlapping units that cover every input byte exactly once.

- The fixed window limit is 8,192 bytes.
- A boundary never splits a UTF-8 code point.
- Within the final 1,024 bytes before a limit, a newline is preferred, then
  other Unicode whitespace.
- If neither exists, the nearest valid boundary at or before the limit is used.
- IDs are `u000000`, `u000001`, and so on.
- Each unit carries its start byte, end byte, and exact text in the prompt.

These units exist only to transport bounded pieces of one stream. Their
boundaries do not create nodes, and concepts may cross them. The same unit may
ground several incomparable nodes.

### Resolution policy

The request contains three hard maxima:

| Value | Default | Meaning |
| --- | ---: | --- |
| `node_budget` | 32 | Maximum nodes, including the root |
| `max_depth` | 6 | Maximum depth, with the root at zero |
| `max_children` | 6 | Maximum immediate children of one node |

They are ceilings, not requested shapes. The model may use fewer nodes, may
stop branches at different depths, and must not add filler merely to consume a
limit. A node is split only when at least two useful narrower refinements are
supported.

## Codex process boundary

The executable embeds `bundles/codex/agent.sh` and
`generated-tree.schema.json`. For each invocation it materializes those assets,
creates an empty working directory, and starts the launcher with piped standard
input, output, and error streams. The launcher executes:

```sh
codex exec \
  --ephemeral \
  --ignore-user-config \
  --ignore-rules \
  --disable shell_tool \
  --disable unified_exec \
  --skip-git-repo-check \
  --sandbox read-only \
  --color never \
  --model gpt-5.6-terra \
  -c 'model_reasoning_effort="medium"' \
  --output-schema generated-tree.schema.json \
  -
```

The installed `codex` executable and its existing authentication are required.
User configuration and repository instruction files do not alter generation.
Shell and unified-execution tools are disabled for the turn. The prompt also
tells the model to treat corpus text as untrusted evidence, avoid other tools
and browsing, and ignore instructions contained in the corpus.

The parent writes the complete prompt to stdin while draining stdout and
stderr concurrently. Human mode forwards terminal-safe progress; JSON mode
buffers it so an error remains one JSON document. The final 64 KiB is retained
for failures. The child starts in a dedicated process group, and timeout or
output-limit failures terminate the whole group. The process must exit
successfully within 30 minutes; stdout must be nonempty UTF-8 and no larger
than 16 MiB. Stdout is parsed as the single JSON proposal without a JSONL
translation layer.

No SQLite write transaction is held while the model runs.

## Prompt and proposal contract

The versioned prompt receives ordered units plus the resolution policy. It asks
the model to consider the complete input, disregard unit boundaries when
choosing concepts, and emit nodes in depth-first preorder.

The output shape is:

```json
{
  "schema_version": 1,
  "nodes": [
    {
      "id": "n0",
      "parent_id": null,
      "text": "Conceptual family",
      "support_unit_ids": ["u000000"]
    }
  ]
}
```

Proposal IDs are local to that response. Database integer IDs are assigned only
after acceptance.

## Deterministic acceptance

Annals rejects a proposal without repairing it when any of these checks fail:

- schema version is not `1`;
- IDs are not exactly `n0`, `n1`, and so on in array order;
- `n0` is not the sole root;
- a parent is missing, follows its child, or violates depth-first preorder;
- any hard maximum is exceeded;
- a node string is empty or has outer whitespace;
- normalized sibling strings duplicate one another;
- an internal node has exactly one child;
- a leaf has no support link;
- a support ID is unknown or repeated on one node;
- the same unit is attached to an ancestor and its descendant;
- input units are not consecutive, complete, nonempty UTF-8 ranges.

The check does not require every input unit to be cited. It also does not score
whether one string is conceptually narrower than another; that remains the
model's judgment.

## Commit boundary

After acceptance, one immediate SQLite transaction inserts:

1. the unchanged raw input and SHA-256 digest;
2. generation metadata and accepted proposal JSON;
3. all input-unit IDs and byte ranges;
4. every node and ordered parent edge;
5. every node-to-unit support link;
6. one derived search row per node;
7. one library revision increment.

Any failure rolls back the complete write. The accepted JSON, generated tree,
grounding, and search index therefore become visible together.

## Tree lifecycle

Generated nodes carry their generation-run ID. Individual add, edit, move, and
delete commands reject any generated tree. `tree delete` removes the complete
generated tree and its retained generation record atomically.

Manual trees use the same homogeneous node representation with a null
generation-run ID. Manual node commands can add, replace, move, or delete their
nodes while preserving a forest, sibling order, and acyclicity. A root is moved
or removed only through tree commands, and a subtree cannot move between roots.

## Search and validation

Canonical nodes and generation records are authoritative. `search_units`, the
FTS5 table, and index-version metadata are derived. Mutations rebuild the
derived rows in the same transaction; `annals reindex` recreates them from
canonical node text and breadcrumbs.

`annals validate` checks SQLite integrity, foreign keys, FTS integrity, tree
structure, raw-input digests, complete UTF-8 unit coverage, generation
ownership, exact reproduction of the recorded adapter output, grounding
references, agreement with the accepted proposal, and exact agreement between
canonical nodes and derived search rows. It reports failures without repairing
them.

# Data model

`schema.sql` is the authoritative SQLite schema. Canonical rows describe trees
and accepted generations. Search rows are derived and rebuildable.

## Canonical tables

### `library_state`

The singleton row stores `revision`. It starts at zero and increments once for
each committed canonical mutation. Reads, validation, backup, and reindexing do
not increment it.

### `raw_inputs`

| Column | Meaning |
| --- | --- |
| `id` | SQLite integer identifier |
| `text` | Unchanged accepted UTF-8 input |
| `sha256` | Lowercase 64-character SHA-256 hex digest |
| `created_at` | UTC RFC 3339 creation time |

The digest is validated by `annals validate` against the retained bytes.

### `generation_runs`

One row records the reproducibility boundary for one accepted model proposal:

- retained-input ID and generated root ID;
- adapter name and version;
- model and reasoning effort;
- prompt version and output-schema version;
- `node_budget`, `max_depth`, and `max_children`;
- the complete accepted proposal encoded as valid JSON;
- UTC RFC 3339 creation time.

The current recorded constants are `raw-window` version `1`,
`gpt-5.6-terra`, medium reasoning, `prompt-v1`, and proposal schema version `1`.

### `nodes`

| Column | Meaning |
| --- | --- |
| `id` | Stable SQLite integer identifier |
| `parent_id` | Parent node, or null for a root |
| `generation_run_id` | Owning generation, or null for a manual node |
| `text` | The one canonical, nonempty, trimmed node string |
| `position` | Nonnegative sibling-order value |
| `created_at`, `updated_at` | UTC RFC 3339 timestamps |

There is no node-kind column. Root, internal node, and leaf are derived from
parent/child relationships.

The adjacency list is a forest. Foreign keys require existing parents and
cascade deletion down a subtree. Partial unique indexes require one sibling
position within each parent and one position among roots. Application checks
reject cycles, root moves through node commands, and moves across roots.

All nodes in an accepted generated tree carry the same nonnull
`generation_run_id`. The run owns exactly one root. The application rejects
individual mutations to those nodes.

### `input_units`

Each row belongs to a generation run and stores:

- a stable ID such as `u000000`;
- a start byte and exclusive end byte into `raw_inputs.text`.

Ranges are nonempty. For an accepted run they are ordered, contiguous,
non-overlapping, valid UTF-8 boundaries, and cover the complete retained input.
Unit text is recovered by slicing the retained input; it is not duplicated in
this table.

### `node_support`

Each row links one generated node to one input-unit ID in the same run. Composite
foreign keys prevent a link from naming a node or unit outside that run. The
primary key prevents duplicate links.

Proposal acceptance additionally requires every leaf to have support, rejects
an unknown unit, and forbids attaching one unit to both an ancestor and its
descendant. A unit may support several incomparable nodes. Coverage of every
input unit is not required.

## Derived search tables

### `search_units`

There is exactly one row per canonical node:

- node ID;
- exact node text and its normalized form;
- complete root-to-node breadcrumb and its normalized form;
- a deterministic content hash;
- indexer version.

Normalization trims outer Unicode whitespace, applies NFKC and lowercase, and
collapses internal whitespace to one ASCII space. It does not remove
punctuation.

The content hash covers the node ID, text, normalized text, breadcrumb,
normalized path, and indexer version. A parent edit or subtree move changes
descendant breadcrumbs, so the current implementation rebuilds every search
row for each canonical mutation.

### `search_fts` and `index_metadata`

`search_fts` is an external-content FTS5 table over node text and breadcrumb.
Insert, update, and delete triggers mirror `search_units`. The tokenizer is
`unicode61 remove_diacritics 2` with two-, three-, and four-character prefix
indexes.

`index_metadata` records the current deterministic indexer version. Search
refuses to run when the version or one-row-per-node count is not current.
`annals reindex` deletes and recreates `search_units`, asks FTS5 to rebuild, and
records the current version.

## Generation commit

Model invocation and proposal validation occur before the database write. Once
accepted, one `BEGIN IMMEDIATE` transaction performs this sequence:

1. insert the retained raw input and digest;
2. insert the generation-run record and accepted JSON;
3. insert every input-unit range;
4. insert generated nodes in proposal preorder, resolving local IDs to SQLite
   IDs;
5. insert support links and assign the run root;
6. rebuild search rows and FTS state;
7. increment the library revision once;
8. commit.

An error rolls back all eight operations.

## Other mutations

Manual root creation, node addition, text replacement, subtree movement, and
deletion also run in one immediate transaction with search rebuild and one
revision increment.

Deleting a manual tree cascades through its nodes. Deleting a generated tree
removes its generation run; node, input-unit, and support rows cascade, and the
retained input is removed when no run references it.

`annals backup` uses SQLite's backup API for a consistent copy and never
replaces an existing output path.

## Validation boundary

The database directly enforces nonnull values, foreign keys, valid numeric
ranges, sibling-position uniqueness, valid accepted JSON, and search ownership.
The application enforces forest acyclicity, generated-tree ownership,
immutability, proposal preorder, resolution maxima, unary-node rejection,
support placement, and UTF-8 range coverage.

`annals validate` independently checks SQLite and FTS integrity, foreign keys,
node strings and parent chains, sibling positions, generation ownership,
retained-input digests, exact reproduction of recorded adapter units,
agreement with the accepted proposal, and derived-row equality. Validation is
read-only with respect to canonical and indexed content.

# Search design

Annals exposes bounded lexical retrieval over concept labels and graph ancestor
context:

```text
annals search QUERY [--at REVISION] [--within cN]
  [--limit N] [--cursor TOKEN]
```

The query must be nonempty after normalization, contain at most 512 UTF-8
bytes and 16 normalized terms, and the limit must be positive.
Search reads HEAD by default; `--at` selects an immutable historical revision.
`--within cN` narrows candidates to the graph below one concept at that
revision.

## Identity and context

Concept labels may repeat, so a result is identified by its durable public
`cN` ID. A shared concept is one result even when several parent routes reach
it. Search never constructs or returns a preferred root-to-concept path.

For retrieval, each concept receives one context made from the deduplicated
labels of all of its ancestors. Thus a query can find a narrowly named concept
through any broader scope above it. Ancestor context is a search aid, not an
additional parent edge or identity rule. Use `concept show`, `concept parents`,
or `graph` to inspect the actual relationships behind a match.

## Matching

Labels, ancestor context, and queries use deterministic normalization:

1. apply Unicode NFKC;
2. apply Unicode lowercase expansion; and
3. collapse Unicode whitespace to one ASCII space.

Punctuation is retained. The normalized query is split on whitespace. A
concept is a candidate when query terms occur in its normalized label or
normalized ancestor context. Exact label matches rank first, followed by label
prefix matches, then broader label-term coverage. Public ID is the final
deterministic tie breaker.

This is a compact lexical lookup, not a semantic-similarity or truth claim. The
liaison uses the same graph concepts through its revision-scoped,
independently paginated `corpus_search` tool.

## Results and pagination

JSON has the language-level shape:

```json
{
  "revision": 12,
  "query": "database locking",
  "within": {"id": "c7", "label": "Database systems"},
  "results": {
    "items": [
      {
        "concept": {
          "id": "c42",
          "label": "Predicate locking",
          "parent_count": 2,
          "child_count": 1,
          "evidence_count": 3,
          "root": false,
          "leaf": false,
          "shared": true
        }
      }
    ],
    "page": {
      "limit": 10,
      "returned": 1,
      "total": 37,
      "next_cursor": "..."
    }
  }
}
```

No matches is successful and returns an empty `items` array. Human output shows
the public ID and label plus compact graph/evidence counts. Two equal labels
remain two results when their IDs differ.

Page metadata reports the requested limit, returned count, and total count.
`next_cursor` is omitted when no further result exists. A cursor is opaque and
valid only for the same library, query, `within` scope, and resolved revision.
A later page may request a different limit. Paging order is deterministic but
has no conceptual meaning.

## Execution

Search runs against the selected revision's immutable relational concepts and
edges. For each normalized term, a recursive query starts at matching labels
and propagates that match to descendants while deduplicating `(term, concept)`
states. Candidates must receive every term. This preserves ancestor-context
semantics without materializing transitive ancestor sets or joined context
strings in Rust.

Only the requested page is converted into owned response objects. A shake
leaves matching and ranking unchanged because it preserves reachability;
direct-relationship counts and the response revision may still change.

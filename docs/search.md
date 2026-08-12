# Search design

Annals search is local lexical retrieval over generated or manual node strings.
SQLite remains authoritative and FTS5 is derived. Raw ingestion text and
support links are retained for grounding but are not searched directly.

## Indexed representation

Every node contributes exactly one `search_units` row containing:

```text
node_id
text
normalized_text
breadcrumb
normalized_path
content_hash
indexer_version
```

The breadcrumb joins every node string from the root through the indexed node
with ` / `. Ancestor strings provide context without copying descendants into
ancestors.

Normalization is shared by indexing and exact lookup:

1. trim outer Unicode whitespace;
2. apply Unicode NFKC;
3. apply Unicode lowercase expansion;
4. collapse internal Unicode whitespace to one ASCII space.

Punctuation remains significant. Canonical text is retained for display.

The external-content FTS5 table indexes two fields:

```sql
CREATE VIRTUAL TABLE search_fts USING fts5(
    text,
    breadcrumb,
    content = 'search_units',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3 4'
);
```

BM25 gives node text weight `8.0` and breadcrumb weight `1.0`.

## Command surface

```text
annals search QUERY [--within NODE_ID] [--limit N] [--explain]
```

The default limit is 10; accepted values are 1 through 100. Query text is
limited to 4,096 UTF-8 bytes and 32 parsed terms.

Whitespace separates terms. Double quotes preserve a phrase. An unterminated
quoted phrase is rejected. Annals constructs and binds the FTS expression;
user punctuation is never passed through as an FTS operator language.

`--within` is a hard scope over the named node and its descendants. The scope
is applied in the recursive SQL query before the candidate limit.

## Retrieval

One query uses a small fixed sequence:

1. Collect exact integer-ID, normalized complete-breadcrumb, and normalized
   complete-text matches.
2. Run an FTS expression joining all terms with `AND`.
3. If fewer candidates than the requested limit exist and there are several
   terms, run a lower-priority `OR` expression.
4. If the result remains sparse, allow a prefix on the final unquoted term when
   it is alphanumeric and at least three characters long.

The candidate pool is ten times the requested limit, clamped to 50 through
500. Candidates are deduplicated by node ID.

Ordering is deterministic:

1. exact identifier or breadcrumb;
2. exact node text;
3. lexical candidates by bounded retrieval-rank score;
4. node ID as the final tie breaker.

The implementation does not infer semantic similarity, propagate scores
through descendants, or rerank by node depth. Tree structure is used for
breadcrumbs and hard subtree scope.

## Results

Each result contains:

- rank and stable node ID;
- canonical node string;
- root-to-node breadcrumb with IDs;
- an optional FTS-highlighted snippet;
- match reasons;
- optional diagnostics from `--explain`.

Human rendering escapes stored control characters. Highlight markers become
terminal color only when color is enabled; JSON output removes the markers.

`--explain` exposes implementation details such as exact class, FTS retrieval
path, lexical rank, raw BM25 value, direct score, branch key, candidate counts,
and fallback use. These diagnostics are intentionally not a stable scoring API.

## Index lifecycle

`search_units`, `search_fts`, and `index_metadata` are derived. Each canonical
mutation rebuilds the complete set inside the same transaction, so committed
node text and search results agree. `annals reindex` performs the same
deterministic rebuild without changing the library revision.

Search checks the indexer version and requires exactly one derived row per
node. When those checks fail, it returns `reindex_required` instead of querying
stale data. `annals validate` also compares every derived row with its canonical
node and breadcrumb and runs the FTS5 integrity check.

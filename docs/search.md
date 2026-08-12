# Search design

Annals exposes simple local retrieval over current concept labels and complete
paths:

```text
annals search QUERY [--limit N]
```

The default limit is 10. The query must be nonempty after normalization and the
limit must be at least one. Search reads HEAD only; use `show --at REVISION` to
inspect historical state.

## Matching

The same deterministic normalization is used for labels, paths, work names,
and queries:

1. apply Unicode NFKC;
2. apply Unicode lowercase expansion; and
3. collapse Unicode whitespace to one ASCII space.

Punctuation is retained. The normalized query is split on whitespace. A
concept is a candidate when at least one query term occurs in its normalized
label-or-path text. Results sort by:

1. exact normalized label or complete-path match;
2. number of matching query terms; and
3. stable internal order as a deterministic tie breaker.

This is intentionally a compact lexical lookup, not a semantic similarity
claim. The liaison performs its own revision-scoped `corpus_search` through the
bounded model tool interface.

## Results

JSON has the language-level form:

```json
{
  "query": "predicate locking",
  "results": [
    {
      "path": [
        "Database systems",
        "Concurrency control",
        "Predicate locking"
      ],
      "label": "Predicate locking",
      "evidence": [
        {
          "work": "Serializable execution",
          "quote": "Predicate locks prevent inserts that change a predicate result."
        }
      ]
    }
  ]
}
```

No matches is successful and returns an empty array. Human output renders the
complete path and evidence count. Neither form exposes concept identities or
source ranges.

## Derived projection

`concept_search` stores one rebuildable row per current concept with exact and
normalized label and path, a deterministic hash, and indexer version. The
external-content `concept_fts` table is kept consistent by triggers.

The current matcher reads canonical corpus state, but search still requires the
derived projection to be current. This makes stale derived state explicit and
keeps validation and future retrieval changes behind one versioned boundary.
When the row count or indexer version is stale, search returns
`reindex_required`.

Applying a change or revert rebuilds the projection in the same transaction.
`annals reindex` rebuilds it without advancing the corpus revision.

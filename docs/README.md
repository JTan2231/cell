# Annals documentation

Annals reconciles one immutable work with one frozen view of a conceptual
corpus through a provisional, evidence-grounded interpretation.

```text
immutable work + corpus revision
              |
       bounded inspection
              |
              v
    best-current reconciliation
              |
     resolve / validate / apply
              |
              v
 recorded interpretation or revision
```

The implemented contracts are:

- [CLI](cli.md): human commands, reconciliation JSON, and output behavior;
- [Architecture](architecture.md): liaison tools, resolution, transactions,
  and revision history;
- [Data model](data-model.md): canonical, examination, history, and derived
  SQLite state;
- [Search](search.md): current label-and-path retrieval;
- [Runtime characteristics](performance-results.md): enforced limits and cost
  shape, without unsupported benchmark claims.

## Terminology

**Library**
: One SQLite database managed by Annals.

**Work**
: A named, immutable, retained UTF-8 source object.

**Concept**
: A corpus-owned label in an ordered forest. A concept is addressed publicly by
  its complete root-to-concept path at a particular revision.

**Evidence**
: An exact quotation from a retained work supporting one concept. Exact source
  ranges are resolved and stored internally.

**Reconciliation**
: One strict language-level request with a summary, one or more semantic
  operations, and optional inert annotations. The host scopes it to a work and
  base revision. It is a provisional interpretation, not a claim of unique or
  final semantic decomposition.

**Resolved reconciliation**
: The validated semantic operations and complete projected corpus resulting
  from a reconciliation. The host compares that projection with its base
  mechanically.

**Model run**
: A liaison examination record, including its frozen context and tool-call
  transcript. It is not a corpus revision.

**Commit**
: An accepted state transition. Commits are linear and append-only; their
  revision numbers are the public history addresses.

**HEAD**
: The current materialized corpus revision.

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
- [Search](search.md): revision-scoped label and ancestor-context retrieval;
- [Runtime characteristics](performance-results.md): enforced limits and cost
  shape, without unsupported benchmark claims.

## Terminology

**Library**
: One SQLite database managed by Annals.

**Work**
: A named, immutable, retained UTF-8 source object.

**Concept**
: A corpus-owned semantic identity addressed by a durable public ID such as
  `c42`. Its label is descriptive and need not be unique.

**Parent edge**
: An explicit broader-to-narrower relationship between two concepts. Parent
  edges form an unordered directed acyclic graph. A concept may have several
  parents; none is primary.

**Root / leaf**
: A root has no parents and a leaf has no children. Both are derived from the
  current edge set rather than stored as placements. Every leaf must have
  evidence.

**Evidence**
: An exact quotation from a retained work supporting one concept. Exact source
  ranges are resolved and stored internally. Evidence supports the concept
  across all of its parent relationships; it is not attached to an edge.

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

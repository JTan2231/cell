# Annals documentation

Annals integrates one immutable work into one frozen view of a conceptual
corpus through one coherent, evidence-grounded change request.

```text
immutable work + corpus revision
              |
       bounded inspection
              |
              v
       complete proposal
              |
     resolve / validate / apply
              |
              v
       next corpus revision
```

The implemented contracts are:

- [CLI](cli.md): human commands, change JSON, and output behavior;
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

**Proposal**
: One strict language-level `change` or `no_change` request, scoped by the host
  to a work and base revision.

**Resolved change**
: The validated semantic operations and complete projected corpus resulting
  from a proposal.

**Model run**
: A liaison examination record, including its frozen context and tool-call
  transcript. It is not a corpus revision.

**Commit**
: An accepted state transition. Commits are linear and append-only; their
  revision numbers are the public history addresses.

**HEAD**
: The current materialized corpus revision.

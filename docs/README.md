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

Use the [project vocabulary](vocabulary.md) for shared contributor and
conversational terminology. It is documentation guidance, not liaison runtime
input or an implemented contract.

The implemented contracts are:

- [CLI](cli.md): human commands, reconciliation JSON, and output behavior;
- [Architecture](architecture.md): liaison tools, resolution, transactions,
  and revision history;
- [Data model](data-model.md): canonical, examination, history, and derived
  SQLite state;
- [Search](search.md): revision-scoped label and ancestor-context retrieval;
- [System installation](system-installation.md): filesystem inbox operation,
  configuration, and systemd or launchd scheduling;
- [Runtime characteristics](performance-results.md): enforced limits and cost
  shape, without unsupported benchmark claims.

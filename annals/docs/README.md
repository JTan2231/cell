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
       stage / correct
              |
      resolve / validate
       /              \
 no corpus effect   transition
       |              |
       v              v
   recorded        pending -- apply --> revision
```

Use the registered Semantics repository `annals` for shared contributor and
conversational terminology. Discover its read contract through
`semantics.repository.explore`, then query it with `semantics repository show
annals`. The documents below remain authoritative for implemented behavior,
and Semantics repository output is never liaison runtime input.

The implemented contracts are:

- [CLI](cli.md): human commands, reconciliation JSON, and output behavior;
- [Architecture](architecture.md): liaison tools, resolution, transactions,
  and revision history;
- [Data model](data-model.md): canonical, examination, history, and derived
  SQLite state;
- [Consumption telemetry](telemetry.md): live per-delivery token accounting,
  observation coverage, and account-wide Codex budget reads;
- [Search](search.md): revision-scoped label and ancestor-context retrieval;
- [System installation](system-installation.md): filesystem inbox operation,
  configuration, and systemd or launchd scheduling;
- [Runtime characteristics](performance-results.md): enforced limits and cost
  shape, without unsupported benchmark claims.

The [Annals provider](../chancery/annals/provider.json) and independently
versioned [Annals Usage provider](../chancery/annals-usage/provider.json)
publish their release-matched installed outward contracts. After discovery
selects an exact entry, `chancery resolve ENTRY_ID` assembles the provider
scope, normalized facets, dependency contracts, exact basis, and explicit
gaps. Annals Usage remains authority only for its live projection, not for
Annals or Nucleus records.

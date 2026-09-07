# Annals documentation

Annals interprets one immutable work against one frozen corpus revision.
It records that interpretation with concepts, relationships, and exact source
quotations.

```text
immutable work + corpus revision
              |
       bounded inspection
              |
              v
    reconciliation request
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
- [Krisis decision-account exchange](../chancery/annals/manuals/decision-account-exchange.md):
  producer-idempotent acceptance into a dedicated library and its bounded
  read-only feed;
- [Consumption telemetry](telemetry.md): live per-delivery token accounting,
  observation coverage, and account-wide Codex budget reads;
- [Search](search.md): revision-scoped label and ancestor-context retrieval;
- [System installation](system-installation.md): filesystem inbox operation,
  configuration, and systemd or Clockwork scheduling;
- [Runtime characteristics](performance-results.md): enforced limits and cost
  shape, without unsupported benchmark claims.

The [Annals provider](../chancery/annals/provider.json) and independently
versioned [Annals Usage provider](../chancery/annals-usage/provider.json)
publish contracts that match their installed releases. Select an entry, then
use `chancery resolve ENTRY_ID` to read its provider scope, normalized facets,
dependency contracts, basis, and gaps. Annals Usage owns its live projection.
Annals and Nucleus own their records.

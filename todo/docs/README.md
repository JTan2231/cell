# Todo documentation

Todo preserves the distinctions between originating intent, identity,
observed state, and desired state:

```text
source + direction
       |
       v
 cN concern ---- research ----> rN pending routing proposal
                                      |
                            explicit decision
                                      |
                  +-------------------+-------------------+
                  |                                       |
          attach/create/revise/unify                 dismiss/defer
                  |
                  v
          tN durable umbrella
                  |
          dated assessment
                  v
       aN situation assessment
                  |
          design reconciliation
                  v
       dN proposed design
                  |
          explicit decision
                  v
        accepted or rejected
```

The diagram stops at accepted design. Todo does not turn designs into plans,
work items, or implementation execution records. Nucleus runtime records for
the research liaisons are provenance, not evidence that a design was
implemented.

The implemented contracts are:

- [CLI](cli.md): commands, authorization provenance, selectors, migration,
  email configuration, and output behavior;
- [Research liaisons](liaison.md): the routing, situation, and design research
  boundaries and their managed tools;
- [Architecture](architecture.md): ownership, runtime integration,
  stale-basis checks, and failure semantics;
- [Data model](data-model.md): durable identities, immutable revisions,
  decisions, and version-1 migration;
- [macOS installation](system-installation.md): user-owned deployment,
  migration rollback, the daily email LaunchAgent, and recovery.

The product-owned [Chancery provider](../chancery/provider.json) indexes these
public use, operation, and development promises. Discover semantically with
`chancery list` and `chancery show`; once one exact entry is selected, run
`chancery resolve <ENTRY_ID>` to assemble its provider scope, normalized
boundary claims, documentation dependency closure, exact basis, and explicit
gaps. Keep documentary resolution separate from live readiness, authorization,
and domain success.

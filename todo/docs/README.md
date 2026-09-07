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

Read the applicable contract:

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
public use, operation and development contracts. Use `chancery list` to find
relevant entries and `chancery show` to read them. After selecting one, run
`chancery resolve <ENTRY_ID>` for its scope, boundary, documentation dependencies,
exact basis and explicit gaps. Resolution does not check runtime readiness,
authorize operations or determine domain success.

- [Rust interface](rust-api.md): provider-owned request and response types,
  boundary decoding, and the local CLI client.

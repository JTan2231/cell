# CRM documentation

- [Architecture](architecture.md): ownership, intake, immutable revisions, and
  the hidden steward lifecycle.
- [CLI](cli.md): commands, input transport, stages, output, and recovery.
- [Data model](data-model.md): schema-one records, identities, transactions,
  idempotency, and migration boundary.
- [macOS user installation](system-installation.md): content-addressed
  deployment, private state paths, verification, rollback, and canary.
- [Chancery provider bundle](../chancery/provider.json): the complete supported
  CRM promise inventory for this release. Use `chancery show ID` for the full
  contract and `chancery resolve ID` for its normalized outward boundary,
  exact basis, dependencies, and explicit gaps.

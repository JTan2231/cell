# CRM documentation

- [Architecture](architecture.md): ownership, profiles, intake, immutable revisions, and
  the hidden steward lifecycle.
- [CLI](cli.md): commands, input transport, stages, output, and recovery.
- [Data model](data-model.md): schema-two records, identities, transactions,
  idempotency, and migration boundary.
- [macOS user installation](system-installation.md): content-addressed
  deployment, private state paths, verification and rollback.
- [Chancery provider bundle](../chancery/provider.json): all supported CRM
  contracts for this release. Use `chancery show ID` for the full contract.
  Use `chancery resolve ID` for its boundary, basis, dependencies and explicit gaps.

- [Rust interface](rust-api.md): provider-owned request and response types,
  boundary decoding, and the local CLI client.

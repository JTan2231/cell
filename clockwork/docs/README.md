# Clockwork documentation

- [Architecture](architecture.md): authority, registration, binding,
  activation, and launchd boundaries.
- [CLI](cli.md): public commands, manifest shape, output, and failure
  semantics.
- [Data model](data-model.md): immutable definitions, stable bindings, and
  activation history.
- [macOS user installation](system-installation.md): content-addressed
  deployment, diagnosis, rollback, and uninstall boundaries.
- [Semantic seed](semantics-seed.md): project-local definitions prepared for a
  later explicit Semantics registration and seed.
- [Chancery provider bundle](../chancery/provider.json): all supported public
  contracts for this release. Use `chancery show ID` to read a contract and
  `chancery resolve ID` to read its resolved terms and declared gaps.

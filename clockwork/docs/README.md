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
- [Chancery provider bundle](../chancery/provider.json): Clockwork's complete
  supported public promise inventory for this release. Use `chancery show ID`
  for a selected contract and `chancery resolve ID` for its normalized promise
  and explicit gaps.

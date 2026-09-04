# Semantics documentation

- [Architecture](architecture.md): authority, event routing, and reconciliation.
- [CLI](cli.md): project, repository, intake, and readiness commands.
- [Data model](data-model.md): schema 2, revisions, effects, and recovery state.
- [System installation](system-installation.md): macOS deployment, Clockwork schedule
  operation, rollback, and uninstall.

The [Semantics provider bundle](../chancery/provider.json) publishes the
complete supported CLI promise inventory for repository exploration, project
operation, and product development in this release. Use `chancery show ID` for
the full contract and `chancery resolve ID` for its normalized outward
boundary, exact basis, and explicit gaps. Chancery is not a Semantics runtime
dependency; the installed CLI remains usable if its discovery catalog is
unavailable.

# Semantics documentation

- [Architecture](architecture.md): authority, event routing, and reconciliation.
- [CLI](cli.md): project, repository, intake, and readiness commands.
- [Data model](data-model.md): schema 2, revisions, effects, and recovery state.
- [System installation](system-installation.md): macOS deployment, Clockwork schedule
  operation, rollback, and uninstall.

The [Semantics provider bundle](../chancery/provider.json) lists all supported
CLI capabilities for repository exploration, project operation, and development.
Use `chancery show ID` for the full contract. Use `chancery resolve ID` for the
contract, sources, and declared gaps. The installed Semantics CLI remains usable
when the Chancery catalog is unavailable.

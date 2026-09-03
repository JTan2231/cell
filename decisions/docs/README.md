# Decisions documentation

- [Architecture](architecture.md)
- [CLI](cli.md)
- [Data model](data-model.md)
- [macOS installation](system-installation.md)
- [Installed capability provider](../chancery/provider.json): release-matched
  Decisions scope, capability and operation index, and normalized lifecycle
  consumer promise.

After Chancery discovery selects an exact Decisions entry, use `chancery
resolve ENTRY_ID` to assemble its provider scope, boundary facets, dependency
contracts, exact basis, and explicit gaps. Resolution reads documentation; it
does not inspect live Decisions readiness or execute a command.

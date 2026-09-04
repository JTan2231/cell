# Vizier documentation

- [Architecture](architecture.md) describes authority and workflow boundaries.
- [CLI and operation](cli.md) documents supported commands, exact document reads, and terminal recovery outcomes.
- [Persistence and recovery](persistence.md) describes the private ledger,
  review scope and lineage, candidate identity, retries, and restart behavior.
- [macOS installation](system-installation.md) describes the selector-only
  deployment boundary.
- [Vocabulary](vocabulary.md) is the project-local Semantics seed.

The Chancery bundle in `vizier/chancery` is the installed discovery contract.
It documents behavior but is never part of Vizier's runtime path.

# Semantics agent instructions

Semantics-Project: semantics

- This folder participates in the installed Semantics service. Its project
  semantic repository is authoritative for project terminology and semantic
  history. Before analyzing, reviewing, or changing code, tests,
  documentation, or interfaces, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and component documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly; report an unresolved repository rather
  than guessing.
- Keep the repository append-only: current meaning is replayed from immutable
  revisions and typed effects; do not add mutable projections in v1.
- Preserve stable project and concept identities across moves and wording
  changes.
- Nucleus requests use a neutral temporary directory, workspace access `none`,
  no shell, and no web. Semantics owns its immutable tool contract and domain
  transaction.
- Decisions and Conversations are read-only upstreams behind adapters. Chancery
  is discovery documentation, never a runtime dependency.
- Do not deploy or run `release.sh` as a side effect of development. The
  release command commits, tags, and pushes. `./ci.sh` must finish green.

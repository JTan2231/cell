# Agent instructions

Semantics-Project: todo

- Keep changes and architecture simple.
- This folder uses the installed Semantics service. Its project repository
  owns project terminology and its history. Before analyzing, reviewing, or changing code, tests,
  documentation, or interfaces, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and component documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly; report an unresolved repository rather
  than guessing.
- Every code change must leave `./ci.sh` green.

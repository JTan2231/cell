# Agent instructions

Semantics-Project: todo

- Keep changes simple; do not overcomplicate or overarchitect.
- This folder participates in the installed Semantics service. Its project
  semantic repository is authoritative for project terminology and semantic
  history. Before analyzing, reviewing, or changing code, tests,
  documentation, or interfaces, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and component documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly; report an unresolved repository rather
  than guessing.
- Every code change must leave `./ci.sh` green.
- `./ci.sh` has a hard 60-second runtime limit; exceeding it is a CI failure.

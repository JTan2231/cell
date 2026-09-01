# Agent instructions

Semantics-Project: chancery

- Keep changes simple; do not overcomplicate or overarchitect.
- This folder participates in the installed Semantics service. Its registered
  semantic repository is authoritative for project terminology and semantic
  history. Before project analysis or changes, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and project documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly; report an unresolved repository rather
  than guessing.
- Every code change must leave `./ci.sh` green.
- `./ci.sh` has a hard 60-second runtime limit; exceeding it is a CI failure.

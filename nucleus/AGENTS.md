# Agent instructions

Semantics-Project: nucleus

- Keep changes simple; do not overcomplicate or overarchitect.
- This folder participates in the installed Semantics service. Its registered
  semantic repository is authoritative for project terminology and semantic
  history. Before project analysis or changes, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and project documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly; report an unresolved repository rather
  than guessing.
- Before changing the public contract, harness compatibility, persistent state,
  authentication or service lifecycle, deployment, or a requester integration,
  read `docs/operator-manual.md`.
- Update `docs/operator-manual.md` in the same change when any of those
  operational facts, boundaries, or procedures change.

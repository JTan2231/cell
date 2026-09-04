# Agent instructions

Semantics-Project: decisions

- Keep changes simple; do not overcomplicate or overarchitect.
- This folder participates in the installed Semantics service. Its project
  semantic repository is authoritative for project terminology and semantic
  history. Before analyzing, reviewing, or changing code, tests,
  documentation, or interfaces, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and component documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly; report an unresolved repository rather
  than guessing.
- Krisis owns eligible-turn coverage, stable decision-account identity and
  rendering, its durable outbox, and Annals acceptance receipts. Annals owns
  the accepted canonical decisions library; Nucleus owns only bounded agent
  execution. Retained Decisions history is compatibility state, not an active
  digest or review surface.
- Never add a direct Codex fallback or let model output become domain authority.
- Every code change must leave `./ci.sh` green.

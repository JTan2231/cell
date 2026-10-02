# Agent instructions

Semantics-Project: email

- Keep changes simple.
- This folder participates in the installed Semantics service. Its registered
  semantic repository defines project terminology and records its history.
  Before project analysis or changes, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and project documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly; report an unresolved repository rather
  than guessing.
- Email always sends from `Codex <codex@joeytan.dev>` to
  `j.tan2231@gmail.com`; do not add configurable recipients.
- Keep full supported feature explanations in this product's Chancery bundle.
  Keep operation manuals focused on prerequisites, effects, ordered steps,
  stop conditions, and verification. Update the owning feature and affected
  procedures together. Keep repository documentation as entry points, and use
  Chancery identities for installed reading.
- Submit committed code changes through `./ci.sh submit COMMIT` from the Cell
  root and verify the manager job outcome. Follow the root CI instructions.

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
- Every code change must leave `./ci.sh` green.

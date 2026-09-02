# Pratica agent instructions

Semantics-Project: pratica

- Keep Pratica a short-lived local CLI. It is not a daemon, scheduler, source
  crawler, policy engine, implementation agent, or release gate.
- This folder participates in the installed Semantics service. Its project
  semantic repository is authoritative for Pratica terminology and semantic
  history. Before analysis, review, or changes, use Chancery to read
  `semantics.repository.explore` and query the Pratica repository. Code, tests,
  and product documentation remain authoritative for behavior. Do not edit
  Semantics state directly.
- Preserve exact opaque Markdown terms. The database may enforce negotiation
  mechanics, identities, bounds, provenance, and integrity, but must not
  interpret headings, clauses, obligations, or product meaning.
- Pratica owns integration, track, offer, assent, agreement, review, attempt,
  and basis records. A steward owns only its represented system's response;
  target systems retain authority for their implementation and contracts.
- Nucleus owns agent admission and execution. Pratica owns every accepted tool
  call, durable domain transition, duplicate decision, recovery rule, and
  domain-success proof. Never add a direct-Codex fallback.
- A sealed agreement proves party assent to one exact terms snapshot on one
  exact basis. It does not prove implementation, deployment, authorization to
  change another system, whole-integration correctness, or continuing
  applicability after a basis changes.
- Update the CLI, architecture, data model, installation guide, Nucleus
  requester facts, and Chancery contracts together when shared behavior
  changes.
- `release.sh` commits, tags, and pushes, and the macOS deployer changes
  installed selectors. Do not invoke either without separate authority.
- Every code change must leave `./ci.sh` green.

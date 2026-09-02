# Agent instructions

Semantics-Project: weaver

- Keep changes simple; do not overcomplicate or overarchitect.
- This folder participates in the installed Semantics service. Its project
  semantic repository is authoritative for project terminology and semantic
  history. Before analyzing, reviewing, or changing code, tests,
  documentation, or interfaces, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and component documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly; report an unresolved repository rather
  than guessing.
- Before changing Weaver's Nucleus requester contract, persistent operational
  state, service lifecycle, deployment, or compatibility boundary, run
  `/Users/joey/.local/bin/nucleus manual` and update Weaver's operator-facing
  documentation in the same change.
- Every code change must leave `./ci.sh` green.
- `./release.sh` publishes a release by committing, tagging, and pushing. Do
  not invoke it as a build or test command.

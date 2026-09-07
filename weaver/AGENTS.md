# Agent instructions

Semantics-Project: weaver

- Keep changes simple.
- This folder participates in the installed Semantics service. Its project
  semantic repository is authoritative for project terminology and semantic
  history. Before you analyze, review, or change code, tests,
  documentation, or interfaces, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and component documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly. If the repository cannot be resolved,
  report the failure.
- Before you change Weaver's Nucleus requester contract, persistent operational
  state, service lifecycle, deployment, or compatibility boundary, run
  `/Users/joey/.local/bin/nucleus manual` and update Weaver's operator-facing
  documentation in the same change.
- Every code change must leave `./ci.sh` green.
- `./release.sh` publishes a release by committing, tagging, and pushing. Do
  not invoke it as a build or test command.

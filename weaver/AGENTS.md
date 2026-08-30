# Agent instructions

- Keep changes simple; do not overcomplicate or overarchitect.
- Before changing Weaver's Nucleus requester contract, persistent operational
  state, service lifecycle, deployment, or compatibility boundary, run
  `/Users/joey/.local/bin/nucleus manual` and update Weaver's operator-facing
  documentation in the same change.
- Every code change must leave `./ci.sh` green.
- `./ci.sh` has a hard 60-second runtime limit; exceeding it is a CI failure.
- `./release.sh` publishes a release by committing, tagging, and pushing. Do
  not invoke it as a build or test command.

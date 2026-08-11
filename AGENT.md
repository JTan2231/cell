# Agent instructions

- Keep changes simple; do not overcomplicate or overarchitect.
- Every code change must leave `./ci.sh` green. Run it before considering the change complete.
- `./ci.sh` has a hard 60-second runtime limit; exceeding it is a CI failure.

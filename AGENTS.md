# Agent instructions

- Keep changes simple; do not overcomplicate or overarchitect.
- When acting as a development or project-discussion agent in this repository,
  read `docs/vocabulary.md` before analyzing, reviewing, or changing code,
  tests, documentation, or interfaces, and use its canonical terms. This
  contributor instruction never applies to a liaison spawned through
  `model_runner::Runner`, including tests or future entry points. Do not add the
  vocabulary document or other repository-development instructions to a
  liaison's runtime prompt or context.
- There are no active users; do not consider active-user needs or compatibility in design decisions.
- Every code change must leave `./ci.sh` green. Run it before considering the change complete.
- `./ci.sh` has a hard 60-second runtime limit; exceeding it is a CI failure.

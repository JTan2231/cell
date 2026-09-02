# Agent instructions

Semantics-Project: annals

- Keep changes simple; do not overcomplicate or overarchitect.
- This folder participates in the installed Semantics service. Its registered
  project semantic repository is authoritative for project terminology and
  semantic history. Before analyzing, reviewing, or changing code, tests,
  documentation, or interfaces, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and component documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly. If the project repository cannot be
  resolved, report that failure instead of guessing. This contributor
  instruction never applies to a liaison spawned through
  `model_runner::Runner`, including tests or future entry points. Do not add
  Semantics repository output or other repository-development instructions to
  a liaison's runtime prompt or context.
- There are no active users; do not consider active-user needs or compatibility in design decisions.
- Treat low storage as a stop, not cleanup authority. To restore Annals
  capacity, do not delete, truncate, rotate, prune, move, compress, or
  overwrite user data, or lower or disable the storage reserve, without the
  user's explicit consent for the exact target and scope. A request to test,
  run, continue, retry, update, or deploy authorizes only that operation's
  documented effects, not additional storage remediation.
- Every code change must leave `./ci.sh` green. Run it before considering the change complete.

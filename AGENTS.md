# Agent instructions

Semantics-Project: cell

## Repository work

- Before analysis, review, or changes, read `semantics.repository.explore`
  through Chancery and query `cell` at this root or the registered product's
  repository when working deeper.
- Before changing the public contract, harness compatibility, persistent state,
  authentication or service lifecycle, deployment, or a requester integration,
  run `/Users/joey/.local/bin/nucleus manual`. If it is unavailable, read
  [the operator manual](nucleus/docs/operator-manual.md). Follow its change
  playbooks and documentation update rules.
- Preserve the product instructions in nested `AGENTS.md` files.
- Every code change must pass the default `./ci.sh`. Follow
  [CI selection](pipeline/README.md) for scope and full-CI rules.

## Documentation

- Follow the shared [documentation rules](nucleus/docs/operator-manual.md#where-facts-and-changes-belong).
  Keep text that helps readers understand the current system, make a decision,
  perform a task, or interpret a result. Retain supported limits and recovery rules.
- Use the ASD-STE100 Issue 9 house style. Preserve meaning over strict rules,
  including required technical terms, commands, and field names. Give each
  paragraph one subject. Start procedure steps with an action verb.
- Keep the root `README.md` to an introduction, basic CI examples, and
  documentation pointers. Change it only when the user explicitly requests it.
  Keep product READMEs to purpose, a small example or check, and documentation links.

## Design proposal discussions

Use this workflow unless the user specifies otherwise:

- First walk through the simplest proposal that fulfills the request: intended
  behavior, boundaries, and important tradeoffs. After agreement on the direction,
  ground it in the actual system and identify the smallest sufficient implementation.
- Establish the desired endpoint and wait for approval before implementing.
  Once approved, own completion through that endpoint. Keep implementation,
  review, and validation focused on the agreed change, and complete required checks.
- For long-running CI, release, or deployment, use a heartbeat every three minutes.
  Check for actionable failures or completion. Stay quiet while nothing needs action,
  and advance through authorized stages. Resolve routine failures within scope.
  Stop and bring back any issue that requires changing the agreed scope or authority.
- Verify the agreed endpoint, stop the heartbeat, and report the outcome.

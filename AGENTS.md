# Agent instructions

Semantics-Project: cell

## Installed capability discovery

Chancery is solely for Cell. Apply this workflow to requests about Cell or its
products.

- When a Cell request may map to an installed local capability, administrative or
  development operation, or cross-system operational manual—and the relevant
  contract is not already established in the current session—run
  `/Users/joey/.local/bin/chancery list` and compare the request semantically
  with the complete installed catalog's titles and summaries.
- Read every plausible entry's complete installed contract with
  `/Users/joey/.local/bin/chancery show <ENTRY_ID>` before choosing or invoking
  a represented interface. Use the contracts to decide among plausible
  entries, and ask only when a material choice remains. If no listed entry
  fits, proceed normally.
- After selecting an exact entry, run
  `/Users/joey/.local/bin/chancery resolve <ENTRY_ID>` when the request concerns
  the system's complete outward promise or a design reliance. Preserve
  unsupported, unspecified, not-applicable, undeclared, dependency, and
  readiness outcomes as reported; never fill a gap from schemas or
  implementation code.
- Chancery is read-only documentation and discovery. Catalog presence is not live
  readiness, user authorization, execution, or domain success. Invoke any
  selected CLI, skill, browser, computer-use surface, or service separately and
  follow its installed authority, effects, privacy, and recovery contract.
- Do not substitute historical notes, source-tree research, or generic keyword
  assumptions for an available installed Chancery contract. If Chancery itself
  is unavailable, report that discovery failure rather than guessing a local
  system route.

## Repository work

- Before analysis, review, or changes, read `semantics.repository.explore`
  through Chancery and query `cell` at this root or the registered product's
  repository when working deeper.
- Before changing the public contract, harness compatibility, persistent state,
  authentication or service lifecycle, deployment, or a requester integration,
  run `/Users/joey/.local/bin/nucleus manual`. If it is unavailable, read
  [the operator manual](infrastructure/nucleus/docs/operator-manual.md). Follow its change
  playbooks and documentation update rules.
- Preserve the product instructions in nested `AGENTS.md` files.
- Commit each code change and submit it with `./ci.sh submit COMMIT` from the
  Cell root, or `telete submit COMMIT`. Verify the retained job outcome before
  considering the change complete. Installed Telete owns integration,
  validation, bounded repair, deployment, and the outcome email. See
  [CI submission](infrastructure/telete/README.md) and
  [queue operation](infrastructure/telete/chancery/manuals/queue-operate.md).
  Root and product `ci.sh` wrappers use this same path; there is no direct
  check-only CI entry point. Focused tests can support development, but they do
  not replace the manager job.
  New jobs refund each repair invocation's budget point when Git accepts its
  patch and the manager records the private candidate. Failed or rejected
  attempts remain charged for the whole job.

## Documentation

- Follow the shared [documentation rules](infrastructure/nucleus/docs/operator-manual.md#where-facts-and-changes-belong).
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
  Do not sacrifice technical precision in this proposal.
- Establish the desired endpoint and wait for approval before implementing.
  Once approved, own completion through that endpoint. Keep implementation,
  review, and validation focused on the agreed change, and complete required checks.
- For long-running CI, release, or deployment, use a heartbeat every three minutes.
  Check for actionable failures or completion. End your turn while nothing needs action,
  and advance through authorized stages. Resolve routine failures within scope.
  Stop and bring back any issue that requires changing the agreed scope or authority.
- Verify the agreed endpoint, stop the heartbeat, and report the outcome.

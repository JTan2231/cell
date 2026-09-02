# Agent instructions

Semantics-Project: cell

- Keep changes simple; do not overcomplicate or overarchitect.
- This root participates in the installed Semantics service. The `cell`
  semantic repository is authoritative for cross-product terminology and its
  history when work is rooted here; a deeper registered product repository is
  authoritative for that product's terminology. Before analysis, review, or
  change work, read `semantics.repository.explore` through Chancery and query
  the applicable Semantics repository. Code, tests, and product documentation
  remain authoritative for behavior. Do not edit Semantics state directly.
- When a user request may map to an installed local capability or adaptive
  operation and the relevant contract is not already established in the
  current session, run `/Users/joey/.local/bin/chancery list`, compare the
  request semantically with the catalog's titles and summaries, and read every
  plausible entry with `chancery show <ENTRY_ID>` before choosing or invoking
  its separately documented interface. Chancery is read-only discovery: catalog
  presence does not establish live readiness, authorize an effect, execute the
  capability, or determine domain success.
- Before changing the public contract, harness compatibility, persistent state,
  authentication or service lifecycle, deployment, or a requester integration,
  run `/Users/joey/.local/bin/nucleus manual`. If it is unavailable, read
  `nucleus/docs/operator-manual.md`.
- Update `nucleus/docs/operator-manual.md` in the same change when shared
  operational facts, boundaries, or procedures change.
- Preserve product-scoped instructions in nested `AGENTS.md` files.
- Every code change must leave `./ci.sh` green.

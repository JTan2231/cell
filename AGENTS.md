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
- After selecting an exact entry, run `chancery resolve <ENTRY_ID>` when the
  request concerns the system's complete outward promise or a design reliance.
  Preserve unsupported, unspecified, not-applicable, undeclared, dependency,
  and readiness outcomes as reported; never fill a gap from schemas or
  implementation code.
- When the user authorizes a Git implementation whose brief, applicable
  terminology, complete contract units, source basis, and validation gates are
  settled—and the work would otherwise be decomposed among implementation or
  review subagents—use the installed `vizier.implementation.delegate`
  capability instead of spawning ad hoc subagents. Freeze the caller-owned
  inputs and let Vizier plan, implement, independently review, integrate, and
  run the gates.
- Prefer exact sealed Pratica exports when they exist; otherwise freeze
  already-approved design and acceptance text without inventing requirements.
  If inputs are materially unsettled, Vizier is unavailable, or a run returns
  `needs_attention`, stop at the applicable design, Pratica, caller, or Vizier
  recovery boundary; do not silently fall back to direct implementation
  subagents.
- Treat an authorized implementation request as permission to apply Vizier's
  exact successful candidate only when the caller branch still matches the
  run's source basis and the relevant worktree is clean. Stop for reconciliation
  if it has advanced or contains relevant changes. Push, release, and deployment
  remain separate actions requiring their own authority.
- Before changing the public contract, harness compatibility, persistent state,
  authentication or service lifecycle, deployment, or a requester integration,
  run `/Users/joey/.local/bin/nucleus manual`. If it is unavailable, read
  `nucleus/docs/operator-manual.md`.
- Update `nucleus/docs/operator-manual.md` in the same change when shared
  operational facts, boundaries, or procedures change.
- Preserve product-scoped instructions in nested `AGENTS.md` files.
- Every code change must leave `./ci.sh` green.

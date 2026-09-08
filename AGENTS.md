# Agent instructions

Semantics-Project: cell

- Keep changes simple.
- Keep the root `README.md` slim: project introduction, basic CI examples, and
  documentation pointers. Do not update it as part of documentation updates
  unless the user explicitly requests a README change. Put operational details
  in the relevant product or shared documentation.
- Use a concise house style based on ASD-STE100 Issue 9. Preserve meaning over
  strict language rules. Keep required technical terms, commands, and field
  names. Add explanation only when it helps the reader.
- Describe each product's records, their supported interpretation, and their
  operations. State which operation or selected records each collection count,
  timestamp, or failure describes.
- The installed Semantics service maintains terminology and its history.
  For work at this root, use the `cell` semantic repository. For a deeper
  registered product, use that product's semantic repository. Before analysis,
  review, or changes, read `semantics.repository.explore` through Chancery and
  query the applicable repository. Code, tests, and product documentation
  define behavior. Do not edit Semantics state directly.
- If a request can use an installed capability or adaptive operation, first
  establish its contract. If the contract is not established in this session,
  run `/Users/joey/.local/bin/chancery list`. Compare the intended outcome with
  the catalog's titles and summaries. Read each plausible entry with
  `chancery show <ENTRY_ID>` before selecting or using its documented interface.
  Chancery reads documentation. A catalog entry does not establish readiness,
  authorize or execute an action, or determine domain success.
- For a system's complete outward promise or a design reliance, select an exact
  entry and run `chancery resolve <ENTRY_ID>`. Preserve its reported unsupported,
  unspecified, not-applicable, undeclared, dependency, and readiness outcomes.
  Do not fill gaps with assumptions from schemas or implementation code.
- Before changing the public contract, harness compatibility, persistent state,
  authentication or service lifecycle, deployment, or a requester integration,
  run `/Users/joey/.local/bin/nucleus manual`. If it is unavailable, read
  `nucleus/docs/operator-manual.md`.
- If shared operational facts, boundaries, or procedures change, update
  `nucleus/docs/operator-manual.md` in the same change.
- Preserve the product instructions in nested `AGENTS.md` files.
- Every code change must leave `./ci.sh` green.
- Use the default `./ci.sh` for routine validation. Run `./ci.sh --all` only
  when the user explicitly requests full CI. Do not add `--all` on your own.

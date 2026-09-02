# Geste agent instructions

Semantics-Project: geste

- Keep Geste simple: it is a short-lived manual casebook CLI, not a daemon,
  observer, model workflow, or source-system mirror.
- This folder participates in the installed Semantics service. Its project
  semantic repository is authoritative for Geste terminology and semantic
  history. Before analysis, review, or changes, use Chancery to read
  `semantics.repository.explore` and query the Geste repository. Code, tests,
  and product documentation remain authoritative for behavior. Do not edit
  Semantics state directly.
- Preserve immutable episode revisions. Reports, graphs, HEAD selection, and
  search are read-time projections.
- Geste owns episode boundaries and authored interpretation. Conversations,
  Decisions, Semantics, Annals, Git, and other cited systems remain
  authoritative for their source records.
- A verified settlement must remain grounded in a Decisions lifecycle source;
  never infer user authority from assistant prose, file activity, or silence.
- Update the CLI, architecture, data model, installation, and Chancery
  contracts together when shared behavior changes.
- `release.sh` commits, tags, and pushes, and the macOS deployer changes
  installed selectors. Do not invoke either without separate authority.
- Every code change must leave `./ci.sh` green.

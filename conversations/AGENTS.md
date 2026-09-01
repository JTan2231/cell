# Agent instructions

Semantics-Project: conversations

- Keep changes simple; Conversations is a short-lived local CLI and library,
  not a daemon, index, database, or model workflow.
- This folder participates in the installed Semantics service. Its registered
  semantic repository is authoritative for project terminology and semantic
  history. Before project analysis or changes, use Chancery to read
  `semantics.repository.explore` and query Semantics for this folder. Code,
  tests, and project documentation remain authoritative for actual behavior.
  Do not edit Semantics state directly; report an unresolved repository rather
  than guessing.
- Codex App Server is the only conversation-history authority. Never read or
  infer the Codex JSONL or SQLite storage format directly.
- Keep default output limited to normalized user and assistant messages. Tool,
  reasoning, approval, and internal item payloads are outside the public corpus.
- Update the CLI, architecture, installation, and Chancery contracts together
  when their shared behavior changes.
- Every code change must leave `./ci.sh` green within its 60-second deadline.
- `release.sh` publishes a release and the macOS deployer changes installed
  selectors; do not invoke either without the corresponding authorization.

# Explore a Semantics repository

Use this capability to read a participating project's maintained vocabulary.

## Select the project

List registered projects when the project ID is not already stated:

```sh
/Users/joey/.local/bin/semantics project list
```

Do not infer an ID from a folder name and do not read the central SQLite file.
The project record exposes its stable ID, canonical current path, status, and
HEAD revision.

## Read current or historical meaning

```sh
/Users/joey/.local/bin/semantics repository show PROJECT
/Users/joey/.local/bin/semantics repository show PROJECT --revision N
/Users/joey/.local/bin/semantics repository search PROJECT QUERY
```

The result includes stable concept IDs, canonical labels, full meanings,
active or retired state, replacements, and distinctions. The schema 3 view also
includes project identity and the selected revision. Search returns the same
view with only matching concepts. Use stable concept IDs to follow terms across revisions.

For complete replay provenance (grounds, withdrawals, creation/change revisions),
use `semantics repository show PROJECT --provenance [--revision N]`.
For change analysis:

```sh
/Users/joey/.local/bin/semantics repository log PROJECT --from 1
/Users/joey/.local/bin/semantics repository diff PROJECT FROM TO
```

`diff` returns the immutable revisions in the selected range. It is not a
synthetic textual diff.

## Interpret authority

Use the repository as authority for maintained terminology and its history.
Use project source, tests, and current product documentation for actual runtime
behavior. A grounding records why meaning entered or left the repository.

Groundings cite an Annals decisions-library/event/account triple, a
preserved legacy Decisions event/decision pair, or a hashed seed, recording the
source attached to a semantic revision.

All commands here are local and read-only. They do not invoke Annals, Decisions,
Conversations, Nucleus, Chancery, or a network service. Keep normalized
decision provenance and repository meanings inside the local project boundary.

If replay fails, stop. Run Semantics doctor and use the project operation
contract; do not edit SQLite or skip a revision.

Rust callers may import `semantics::api` repository types and use its typed
`Client` for repository show, search, log, and diff. The client invokes the
same local CLI and preserves its read-only effects and project authority.
Project and intake operations are separately documented operational actions.

Project lists return stable ID, canonical current path, status, and HEAD.
`show --provenance` returns the full replay representation. Rust callers use
`RepositoryView` for ordinary reads and `Client::repository_provenance` for full
replay. These output selections do not change the persistent schema or replay behavior.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.

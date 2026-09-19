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
active or retired state, replacements, and distinctions. The schema 2 view also
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

Rust callers use `semantics::api::Client` for the same read-only operations.
Ordinary reads return `RepositoryView`; `Client::repository_provenance` returns
the full replay representation. Output selection changes neither the persistent
schema nor replay behavior. Project and intake operations have separate
operational contracts.

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

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

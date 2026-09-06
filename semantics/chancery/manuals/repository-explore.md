# Explore a Semantics repository

Use this capability when a participating project's maintained vocabulary—not
its runtime implementation—should inform the work.

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

The result includes stable concept IDs, canonical labels and meanings,
active/retired state, replacements and distinctions. The schema-two view
includes project identity and the selected repository revision; search returns
the same view with only matching concepts. No concept meaning is excerpted. Prefer stable concept identity over label matching when
following a term across revisions.

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
behavior. A grounding says why meaning entered or left the repository; it does
not prove that all implementation details remain current.

Groundings may cite an Annals decisions-library/event/account triple, a
preserved legacy Decisions event/decision pair, or a hashed seed. These are
provenance rather than current-force claims.

All commands here are local and read-only. They do not invoke Annals, Decisions,
Conversations, Nucleus, Chancery, or a network service. Keep normalized
decision provenance and repository meanings inside the local project boundary.

If replay fails, stop. Run Semantics doctor and use the project operation
contract; do not edit SQLite or skip a revision.

Rust callers may import `semantics::api` repository types and use its typed
`Client` for repository show, search, log, and diff. The client invokes the
same local CLI and preserves its read-only effects and project authority.
Project and intake operations are separately documented operational actions.

Project lists select stable ID, canonical current path, status and HEAD.
Ordinary repository show/search select schema version 2, project identity,
revision and concepts with ID, label, complete meaning, active state,
replacement and complete distinctions. `show --provenance` retains the full
original full replay representation. Rust callers use `RepositoryView` for the
ordinary read and `Client::repository_provenance` for the full replay.
These are output projections, with no persistent schema or replay change.

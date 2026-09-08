# Operate an Annals library

This capability covers deterministic library administration and explicit
corpus-history changes. It does not operate the scheduled inbox or invoke the
AI reader.

## Named libraries and librarian instructions

```sh
/Users/joey/.local/bin/annals library list
/Users/joey/.local/bin/annals library create conatus
/Users/joey/.local/bin/annals library conatus show
/Users/joey/.local/bin/annals library conatus instructions set 'Organize wants and decisions by the wants they appear to serve.'
/Users/joey/.local/bin/annals library conatus instructions set --file conatus.md
/Users/joey/.local/bin/annals library conatus instructions set --stdin
/Users/joey/.local/bin/annals library conatus instructions show
/Users/joey/.local/bin/annals library conatus instructions history --limit 20
```

A name scopes ordinary Annals commands. The catalog resolves it to one stable
library ID, database, config, and spool. Names contain 1–64 lowercase ASCII
letters, digits, underscores, or hyphens, start with a letter, and cannot be
`list`, `create`, or `help`. Unknown names fail without creation. Creation
reserves one identity in `provisioning`, initializes matching state, and marks
it `ready`. Repeating the same name and kind resumes or returns that library;
conflicting state fails without replacement. Creation starts no model or schedule.

The catalog lives at `ANNALS_STATE_DIR/catalog.db`, defaulting to
`~/Library/Application Support/Annals` on macOS or `~/.local/share/annals`
elsewhere. New state is under `libraries/LIBRARY_ID/`. Existing config/path
selection remains supported and is not automatically registered. Named scope
rejects `--library`, ignores `ANNALS_LIBRARY`, and checks identity before use.
An explicit config may tune execution but cannot redirect the named database,
spool, or admission kind. List reports registrations; show reports registration,
corpus revision, and selected instruction revision. Neither proves runtime readiness.

`instructions set` accepts exactly one nonblank UTF-8 document, preserves its
exact bytes, and atomically appends and selects an instruction revision in the
library database. The receipt contains `changed` and an `instructions` object with `library_id`,
`revision`, `content`, `sha256`, and `recorded_at`. Identical current bytes return unchanged;
A → B → A appends three selections. `recorded_at` describes instruction selection,
not graph reinterpretation. History identifies `library_id` and
`current_instruction_revision` and returns newest-first `instructions` and
`has_more`, default 20 and maximum 100; `--before REVISION` continues strictly
older selections.

Library instructions define what the librarian organizes and what concepts and
parent connections mean. Annals still enforces source immutability, exact
quotations, valid identities, an acyclic graph, and evidence requirements.
The default stored instructions use broader/narrower scope. Instructions are
trusted settings, separate from source evidence; changing them starts no model
run, changes no corpus revision, and rewrites no committed history.

## Initialize, inspect, and back up

```sh
/Users/joey/.local/bin/annals init [--kind general|decisions]
/Users/joey/.local/bin/annals migrate
/Users/joey/.local/bin/annals stats
/Users/joey/.local/bin/annals backup <ABSENT_OUTPUT_PATH>
```

`init` creates revision zero with one immutable library kind and refuses to
replace a path. It defaults to `general`; `--kind decisions` is only for a
physically separate producer-accepted decisions library. `migrate` supports
versions 3 through 5 to schema 6. It assigns version-3 and version-4 libraries
the `general` kind, preserves existing version-5 kinds, and seeds the default
instructions at instruction revision 1 in one transaction. Historical
examination provenance stays null; migration does not invent its instruction
basis or reinterpret the corpus. It refuses unsupported older or newer
libraries without reinterpreting them. Configuration cannot change a library's
kind. `stats` is read-only.
`backup` creates a consistent SQLite copy and refuses to replace its
destination.

Use the selected installed-system deployment procedure for a fresh-state
cutover. Do not approximate one by deleting or editing the active database.

## Direct reconciliations

An expert caller may submit strict reconciliation JSON without a model:

```sh
/Users/joey/.local/bin/annals change submit <REQUEST_JSON> \
  --work <LABEL> --base <REVISION>
/Users/joey/.local/bin/annals change show --work <LABEL>
/Users/joey/.local/bin/annals change validate --work <LABEL>
/Users/joey/.local/bin/annals change apply --work <LABEL>
```

Submission resolves and validates a complete projected corpus state but does
not apply it. It freezes the current instruction revision when submitted.
Application additionally requires HEAD and instructions to match the stored
basis in the committing transaction and atomically updates concepts, edges, evidence, reconciliation status,
history, and revision. There is no force path around stale state.

Existing concepts are selected by public `cN` ID and same-request creations by
local handles. Evidence selectors use exact source quotations and optional
heading or adjacent-text filters. Every resulting leaf requires evidence and
the explicit parent graph must remain acyclic.

## Normalize and revert history

```sh
/Users/joey/.local/bin/annals shake
/Users/joey/.local/bin/annals log
/Users/joey/.local/bin/annals diff <FROM_REVISION> <TO_REVISION>
/Users/joey/.local/bin/annals revert <REVISION>
```

`shake` previews transitive reduction and asks for confirmation unless `--yes`
is explicitly supplied. A confirmed nonempty plan is bound to the exact
library identity, HEAD, and instruction revision, removes only direct parent edges already implied by
longer paths, and creates one commit. It preserves reachability but not all
direct-neighbor counts or hop distances. A direct relationship can carry meaning
under the library instructions even when another path exists.

`revert` applies the inverse of one earlier commit to current HEAD and appends
the result as a new commit. It never erases history. Relevant intervening
changes cause an atomic conflict; unrelated facts survive.

The library and every backup contain retained sources, exact evidence,
reconciliation and model-run provenance, instruction revisions, and complete corpus history. Keep
them private. Application, shake confirmation, revert, migration, installed
fresh-state replacement, and backup placement each require authority
appropriate to their effects.

# Operate an Annals library

This capability covers deterministic library administration and explicit
corpus-history changes. It does not operate the scheduled inbox or invoke the
AI reader.

## Initialize, inspect, and back up

```sh
/Users/joey/.local/bin/annals init
/Users/joey/.local/bin/annals migrate
/Users/joey/.local/bin/annals stats
/Users/joey/.local/bin/annals backup <ABSENT_OUTPUT_PATH>
```

`init` creates revision zero and refuses to replace a path. `migrate` supports
only documented prior schemas, runs transactionally, and refuses older or
newer unsupported libraries without reinterpretation. `stats` is read-only.
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
not apply it. Application additionally requires current HEAD to equal the base
and atomically updates concepts, edges, evidence, reconciliation status,
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
library identity and HEAD, removes only direct parent edges already implied by
longer paths, and creates one commit. It preserves reachability but not all
direct-neighbor counts or hop distances.

`revert` applies the inverse of one earlier commit to current HEAD and appends
the result as a new commit. It never erases history. Relevant intervening
changes cause an atomic conflict; unrelated facts survive.

The library and every backup contain retained sources, exact evidence,
reconciliation and model-run provenance, and complete corpus history. Keep
them private. Application, shake confirmation, revert, migration, installed
fresh-state replacement, and backup placement each require authority
appropriate to their effects.

# Explore the Annals corpus

Annals reads are local and bounded. HEAD is the default. Commands that accept
`--at <REVISION>` can read an immutable historical corpus state. Reads invoke
no model and make no network request.

Use `annals library NAME COMMAND` to select a registered library. Each library
has separate source, graph, and history identity. The library's stored
instructions define the meaning of concepts and parent edges; the default
frame uses broader/narrower conceptual scope. A historical graph can include
commits interpreted under earlier instruction revisions. Reconciliation
inspection reports the used and current instruction revisions separately.
No combined search or transaction across libraries is provided.

## Find and inspect ideas

```sh
/Users/joey/.local/bin/annals overview
/Users/joey/.local/bin/annals roots
/Users/joey/.local/bin/annals search <QUERY>
/Users/joey/.local/bin/annals concept show <CONCEPT_ID>
/Users/joey/.local/bin/annals concept parents <CONCEPT_ID>
/Users/joey/.local/bin/annals concept children <CONCEPT_ID>
/Users/joey/.local/bin/annals concept evidence <CONCEPT_ID>
/Users/joey/.local/bin/annals graph <CONCEPT_ID>
```

Concepts are selected by durable lowercase `cN` IDs. Labels need not be unique
and are never selectors. Concepts have no canonical path, primary parent, or
sibling position.

`search` performs word-based matching over normalized concept labels and
derived ancestor-label context. `--within <cN>` limits the search to concepts
below one selected concept.

Graph expansion is bounded by direction, depth, and maximum nodes. A frontier
identifies where those limits stopped expansion. It does not identify corpus
leaves or contain the complete graph.

Evidence output pairs a concept with exact quotation occurrences from retained
works. Evidence supports the concept across its parent relationships and
belongs to the concept rather than one edge.

## Works, deliveries, and history

```sh
/Users/joey/.local/bin/annals work list
/Users/joey/.local/bin/annals work show <LABEL>
/Users/joey/.local/bin/annals lately
/Users/joey/.local/bin/annals lately --since 30d --status failed --by completed
/Users/joey/.local/bin/annals log
/Users/joey/.local/bin/annals diff <FROM_REVISION> <TO_REVISION>
```

`work show` returns complete retained source text. `lately` instead reports
source-delivery metadata and never searches or emits source content, topics,
or dates mentioned within a work. Its `--by` basis controls both time-window
membership and ordering. A missing ingestion or completion time can omit a
delivery; choose `first-seen` or another available basis when diagnosing early
failures.

The commit log contains applied reconciliations, confirmed shakes, and reverts.
Work retention, recorded no-change interpretations, model runs, and failures
are not corpus transitions. `diff` reports exact concept, edge, and evidence
effects without inventing move or ordering semantics.

Paged cursors are opaque and bound to the selected library, revision, command,
query, and scope. Restart pagination when that context changes. All reads leave
the library unchanged, but their returned labels, quotations, source text, and
history may be private.

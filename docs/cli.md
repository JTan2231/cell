# CLI contract

## Global options

```text
annals [--library PATH] [--json] [--quiet] [-v...] COMMAND
```

The library path resolves from `--library`, then `ANNALS_LIBRARY`, then
`./annals.db`. `--json` emits one success object on stdout or one error object
on stderr. `--quiet` suppresses successful human mutation messages. `-v`
prints the resolved library path on stderr in human mode.

## Library operations

```text
annals init
annals stats
annals validate
annals backup OUTPUT
annals reindex
```

`init` creates revision zero and refuses to replace an existing library.
`stats` reports revision and corpus, graph, work, reconciliation, history,
model-run, database-size, and index information.

`validate` checks SQLite, foreign keys, retained-work digests, current graph
invariants, linear history and its full snapshots, agreement between
materialized HEAD and history, and derived search state. It does not repair
state.

`backup` makes a consistent SQLite copy and refuses to replace its destination.
`reindex` recreates current concept-search rows without advancing the revision.

## Immutable works

```text
annals work add INPUT [--name LABEL]
annals work list
annals work show LABEL
```

`INPUT` is a UTF-8 file containing non-whitespace source text, or `-`. A file
defaults to its UTF-8 filename stem; stdin requires `--name`. Work labels are
nonempty and normalized-unique. Exact retained bytes are content-addressed by
SHA-256. Supplying them again, even with another requested label, selects the
original work and label. A label already attached to different bytes is a
conflict.

Adding a work does not change the corpus revision. Human `work list` shows
labels and sizes; JSON also reports SHA-256 digests and creation times. `work
show` reports that metadata, Markdown heading paths, and the complete unchanged
text. Source heading paths describe the document; they are not concept paths.

## Model-assisted integration

```text
annals integrate INPUT [--name LABEL] [--quality QUALITY] [--model MODEL] [--apply] [--reexamine]
annals integrate --work LABEL [--quality QUALITY] [--model MODEL] [--apply] [--reexamine]
```

The first form retains and examines a new work. The second examines an already
retained work. Annals freezes the current corpus revision, invokes the liaison,
and expects one `submit_reconciliation` tool call. The model's final response
is diagnostic and is not parsed as the reconciliation.

Annals may reuse the newest successful reconciliation for the exact same work,
base revision, prompt version, model, and reasoning effort. `--reexamine`
bypasses this lookup. A later corpus revision or changed liaison configuration
starts a fresh examination.

`--quality` accepts three presets and defaults to `high`:

| Quality | Model | Reasoning effort |
| --- | --- | --- |
| `low` | `gpt-5.6-luna` | `medium` |
| `medium` | `gpt-5.6-terra` | `medium` |
| `high` | `gpt-5.6-sol` | `max` |

`--model` overrides only the model selected by the preset; the selected
quality continues to choose reasoning effort.

The liaison submits a provisional, best-current interpretation. It does not
filter source material by estimated novelty or salience and does not claim an
objective final decomposition into atomic concepts.

Without `--apply`, a reconciliation whose projected graph differs from its
base remains pending. `--apply` immediately commits that pending transition. A
mechanically equal projection is stored with status `recorded`; it creates no
commit and does not advance the revision. Optional annotations are inert and
never block application.

## Reconciliations and corpus changes

```text
annals change submit INPUT --work LABEL --base REVISION
annals change list
annals change show [--work LABEL | --at REVISION]
annals change validate [--work LABEL]
annals change apply [--work LABEL]
```

`change submit` reads strict reconciliation JSON from a file or `-`. The flags
provide the immutable evidence work and frozen corpus revision; both are
deliberately absent from the semantic request.

Submission resolves and validates the complete projected graph but does not
mutate the corpus. A result based on the same or a later revision supersedes
that work's previous pending reconciliation. An older-base result is retained
without displacing a newer pending result. `change list` includes pending,
applied, superseded, and recorded reconciliations.

With `--work`, `change show` selects that work's pending reconciliation when
one exists, otherwise its newest record. Without `--work`, it selects the sole
pending result; when none is pending, it succeeds only if exactly one work has
recorded results. `change validate` and `change apply` select pending results
only and require `--work` when more than one exists.

`change show --at REVISION` retrieves the accepted change at that revision.
For an applied reconciliation it shows the original graph-native request and
resolved operations. For a revert it shows the target revision and resolved
inverse. Both include commit metadata, actor, and timestamp.

Human reconciliation output renders public `cN` IDs alongside labels, local
creation handles, parent-edge changes, exact evidence quotations and source
context, evidence dispositions, replacements, and annotations. `change
validate` re-resolves and renders the same semantic facts without writing.
Resolved operations are request receipts, so an idempotent ensure may appear
there even when it contributes no diff entry.

`change apply` additionally requires HEAD to equal the base revision. Success
updates concepts, edges, evidence, search state, reconciliation status,
history, and revision in one transaction.

### Reconciliation contract

A reconciliation contains a summary, one or more operations, and optional
free-form annotations:

```json
{
  "summary": "Integrate predicate locking and phantom prevention",
  "operations": [
    {
      "action": "add_evidence",
      "concept": {"id": "c12"},
      "evidence": [
        {
          "quote": "A serializable execution has the same effect as some serial execution."
        }
      ]
    },
    {
      "action": "create_concept",
      "ref": "predicate_locking",
      "label": "Predicate locking",
      "parents": [{"id": "c12"}, {"id": "c27"}],
      "evidence": [
        {
          "quote": "Predicate locks prevent inserts that would change the result of a previously evaluated predicate.",
          "within_heading": ["Transactions", "Avoiding phantom reads"]
        }
      ]
    },
    {
      "action": "add_parent",
      "concept": {"id": "c31"},
      "parent": {"new": "predicate_locking"}
    }
  ],
  "annotations": [
    "The work presents predicate locking as a phantom-prevention technique."
  ]
}
```

Every object rejects unknown fields. Summaries, annotations, labels, handles,
and quotations must be nonempty when present. Labels and handles have no outer
whitespace or control characters. `annotations` may be omitted and defaults to
an empty list. Annotations are retained as meta-level context only; they are
not evidence, confidence levels, or review flags and do not affect validation
or application.

### Concept selectors

An existing concept is addressed by its durable public ID:

```json
{"id":"c42"}
```

Public IDs have a lowercase `c` followed by a positive canonical decimal
integer. They preserve identity across rewording and relationship changes.

A concept created in the same request declares a request-unique `ref` and is
selected by that handle:

```json
{"new":"predicate_locking"}
```

Local handles may be referenced anywhere in the request, including before the
corresponding creation appears. They are not labels. Different concepts may
have identical labels, so labels never select a concept.

There are no concept-path selectors. The only path arrays in the public
contract locate headings within source works.

### Evidence

Evidence always belongs to the work supplied by the host:

```json
{
  "quote": "Exact source language",
  "within_heading": ["Optional", "exact Markdown heading path"],
  "preceded_by": "Optional exact neighboring text",
  "followed_by": "Optional exact neighboring text"
}
```

`quote` is required; the other fields disambiguate repeated source text.
Public input never contains source offsets. Once resolved, evidence supports
the concept across all of its parent relationships. Every leaf in the final
projected graph must have at least one evidence link.

### Operations

- `create_concept` requires request-unique `ref`, `label`, an unordered
  `parents` array, and nonempty `evidence`. An empty parent array creates a
  derived root. Labels may duplicate existing or newly created labels.
- `add_parent` ensures one broader-parent edge exists for `concept` without
  changing any other parent. An already-present edge is idempotent.
- `remove_parent` removes one parent edge without relocating the concept or its
  descendants. If it removes the final parent, the concept becomes a root.
- `add_evidence` ensures one or more quotations from the scoped work are
  attached to the selected concept. An already-satisfied mapping is
  idempotent.
- `remove_evidence` removes quotations from the scoped work that are attached
  to the selected concept.
- `reword_concept` preserves the public ID and requires
  `evidence_disposition: "retain" | "remove"`.
- `retire_concept` removes one concept and its incident edges. Retirement is
  nonrecursive: children survive, and a child with no remaining parents
  becomes a root. Optional `replacement` records a semantic successor but does
  not transfer edges or evidence.

The complete projected result must be an acyclic graph with valid endpoints,
no self or duplicate edges, and evidence on every leaf. There is no parent
priority, sibling placement, integer position, path, or move operation.

## Local corpus browsing

Corpus reads are deliberately local and bounded. HEAD is the default; `--at`
selects an immutable historical revision.

```text
annals overview [--at REVISION]
annals roots [--at REVISION] [--limit N] [--cursor TOKEN]

annals concept show cN [--at REVISION] [--preview-limit N]
annals concept parents cN [--at REVISION] [--limit N] [--cursor TOKEN]
annals concept children cN [--at REVISION] [--limit N] [--cursor TOKEN]
annals concept evidence cN [--at REVISION] [--limit N] [--cursor TOKEN]

annals graph cN [--at REVISION] [--direction parents|children|both]
  [--depth N] [--max-nodes N]

annals search QUERY [--at REVISION] [--within cN]
  [--limit N] [--cursor TOKEN]
```

`overview` returns revision-wide counts for concepts, explicit edges, roots,
leaves, shared concepts, and evidence. It does not dump the graph.

`roots` pages through concept summaries with no parents. `concept show` returns
one concept's ID, label, relationship and evidence counts, derived
root/leaf/shared flags, and bounded previews. The `parents` and `children`
subcommands page through compact `{id, label}` references; `evidence` pages
through work-and-quotation pairs.

`graph` performs a bounded local expansion around one concept. `direction`
chooses incoming parent edges, outgoing child edges, or both. Each concept
appears once even when several routes reach it. When depth or node limits cut
off the expansion, the response reports a frontier instead of implying that
the returned neighborhood is complete.

`search` matches labels and ancestor-label context. `--within cN` restricts the
search to the graph below one concept. Search results remain distinct by
public ID when labels repeat.

Paged responses contain `items` plus `page` with the requested limit,
returned count, total count, and optional `next_cursor`. The cursor is omitted
when the page is complete. Cursors are opaque and tied to the same library,
command, query, scope, and resolved revision. A later page may request a
different limit. Deterministic page order is a rendering contract, not a
conceptual ordering.

## History

```text
annals log [--limit N]
annals diff FROM TO
annals revert REVISION
```

`log` lists newest commits first. Work retention, recorded reconciliations,
model runs, and failed attempts are absent because they are not corpus
transitions.

`diff` compares two retained full snapshots and reports concept creation,
retirement, and rewording; individual parent edges added or removed; and
evidence added or removed. It never synthesizes a move or reorder event.

`revert` inverses one earlier commit against current HEAD and creates a new
commit. It does not erase history. If a relevant concept, edge, or evidence
fact has changed since the target transition, it fails atomically with
`revert_conflict`; unrelated relationships survive.

## Output and exit behavior

JSON success and failure envelopes are:

```json
{"ok":true,"data":{}}
```

```json
{"ok":false,"error":{"code":"stable_code","message":"description"}}
```

Public corpus JSON uses `cN` concept IDs, labels, exact quotations, edge
endpoints, opaque pagination cursors, and revision numbers. It does not expose
work, reconciliation, evidence, commit-row, or model-run IDs, nor source byte
ranges. Source-document heading paths remain public where they locate work
text.

Exit categories are:

| Code | Meaning |
| ---: | --- |
| 0 | Success |
| 1 | Unexpected process, I/O, or JSON failure |
| 2 | Invalid command or input |
| 3 | Missing library, work, concept, reconciliation, or revision |
| 4 | Stale state, invariant, or reversion conflict |
| 5 | SQLite, integrity, history, or index failure |

Human rendering escapes control characters from retained text and labels.

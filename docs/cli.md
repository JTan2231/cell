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
`stats` reports corpus revision and concept, work, evidence, pending-change,
commit, and model-run counts, plus database size and index freshness.

`validate` checks SQLite, foreign keys, retained-work digests, current corpus
invariants, linear history and its snapshots, agreement between materialized
HEAD and history, and the derived search projection. It does not repair state.

`backup` makes a consistent SQLite copy and refuses to replace its destination.
`reindex` recreates derived concept search rows without advancing the corpus
revision.

## Immutable works

```text
annals work add INPUT [--name LABEL]
annals work list
annals work show LABEL
```

`INPUT` is a nonempty UTF-8 file or `-`. A file defaults to its UTF-8 filename
stem; stdin requires `--name`. Work labels are nonempty and normalized-unique.
The same retained bytes also cannot be added under another label.

Adding a work does not change the corpus revision. The human `work list` shows
labels and sizes; `work list --json` also reports SHA-256 integrity digests and
creation times. `work show` reports that metadata, Markdown heading paths, and
the complete unchanged text. Labels, not digests or database identifiers,
select works.

Example JSON from `work add`:

```json
{
  "ok": true,
  "data": {
    "work": "Serializable execution",
    "size_bytes": 18420,
    "sha256": "...",
    "created_at": "2026-08-12T12:00:00Z",
    "corpus_revision": 0
  }
}
```

## Model-assisted integration

```text
annals integrate INPUT [--name LABEL] [--apply]
annals integrate --work LABEL [--apply]
```

The first form retains a new work and then examines it. The second examines an
already retained work. Annals freezes the current corpus revision, invokes the
liaison, and expects one recorded `change` or `no_change` result through its
tool interface. The model's final response is not parsed as the proposal.

Without `--apply`, a valid change remains pending. `--apply` immediately
commits a change only when its `uncertainties` array is empty. An uncertain
change remains pending with status `review required`; submit a revised certain
proposal before applying it. A `no_change` examination is recorded but never
creates a corpus commit.

## Changes

```text
annals change submit INPUT --work LABEL --base REVISION
annals change list
annals change show [--work LABEL | --at REVISION]
annals change validate [--work LABEL]
annals change apply [--work LABEL]
```

`change submit` reads strict proposal JSON from a file or `-`. The flags provide
the immutable evidence work and the corpus revision examined by the submitter;
those values are deliberately absent from the semantic proposal.

Submitting resolves and validates the complete transition but does not mutate
the corpus. A result based on the same or a later revision supersedes that
work's previous pending proposal. An older-base result is retained without
displacing a newer pending proposal. `change list` includes pending, applied,
superseded, and no-change records.

With `--work`, `change show` selects that work's pending proposal when one
exists, otherwise its newest record. Without `--work`, it selects the sole
pending proposal; when none is pending, it succeeds only if exactly one work
has recorded results. `change validate` and `change apply` select pending
changes only and require `--work` when more than one exists.

`change show --at REVISION` retrieves the accepted corpus change recorded by
that revision, even when later examinations exist for the same work. For an
accepted proposal it shows the original language-level proposal and resolved
semantic operations. For a revert it shows the target revision and resolved
inverse transition. Both include commit metadata, actor, and timestamp.

Human `integrate`, `change submit`, and `change show` output renders every
requested operation with its language-level paths, parent and relative-order
placement, exact evidence quotations and disambiguating context, evidence
disposition, replacement, and uncertainties. `change validate` re-resolves the
request and renders the resulting paths, quotations, parent and ordering
relations, dispositions, replacements, and uncertainties without writing.

`change apply` additionally requires HEAD to equal the proposal's base revision
and the uncertainty list to be empty. Success updates the current corpus,
search projection, proposal status, commit log, and revision in one transaction.

### Proposal variants

A change contains one or more operations:

```json
{
  "outcome": "change",
  "summary": "Integrate the work's treatment of serializable execution",
  "operations": [
    {
      "action": "add_evidence",
      "concept": {
        "path": [
          "Database systems",
          "Concurrency control",
          "Serializable execution"
        ]
      },
      "evidence": [
        {
          "quote": "A serializable execution has the same effect as some serial execution."
        }
      ]
    },
    {
      "action": "create_concept",
      "label": "Predicate locking",
      "under": {
        "path": [
          "Database systems",
          "Concurrency control",
          "Serializable execution"
        ]
      },
      "evidence": [
        {
          "quote": "Predicate locks prevent inserts that would change the result of a previously evaluated predicate.",
          "within_heading": ["Transactions", "Avoiding phantom reads"]
        }
      ]
    },
    {
      "action": "move_concept",
      "concept": {
        "path": ["Database systems", "Phantom prevention"]
      },
      "under": {"new": "Predicate locking"}
    }
  ],
  "uncertainties": []
}
```

A no-change result has no operations:

```json
{
  "outcome": "no_change",
  "summary": "The work is already represented",
  "reason": "Its distinct claims and evidence are already present.",
  "uncertainties": []
}
```

Every object rejects unknown fields. Summaries, reasons, uncertainties, labels,
paths, and quotations must be nonempty. Concept labels have no outer
whitespace.

### Selectors, evidence, and placement

An existing concept is addressed by its complete path at the base revision:

```json
{"path":["Database systems","Concurrency control"]}
```

A concept created anywhere in the same request is addressed by its label:

```json
{"new":"Predicate locking"}
```

Created labels must be request-global unique after normalization. Root and
sibling labels are also normalized-unique in the projected corpus. Paths are
arrays so punctuation in a label has no structural meaning.

Evidence always belongs to the work supplied by the host:

```json
{
  "quote": "Exact source language",
  "within_heading": ["Optional", "exact Markdown heading path"],
  "preceded_by": "Optional exact neighboring text",
  "followed_by": "Optional exact neighboring text"
}
```

`quote` is required; the other fields disambiguate repeated text. Public input
never contains source offsets.

New and moved concepts append by default. `under` selects the parent;
`before` or `after` optionally selects a sibling ordering anchor. `before` and
`after` cannot both appear. Omitting `under` means root placement. There are no
integer ordering positions in the contract.

### Operations

- `create_concept` requires `label` and nonempty `evidence`; `under`, `before`,
  and `after` are optional.
- `add_evidence` attaches one or more exact quotations from the scoped work.
- `remove_evidence` removes quotations from the scoped work that are already
  attached to the selected concept.
- `move_concept` preserves concept identity and moves its complete subtree.
- `reword_concept` preserves identity and requires
  `evidence_disposition: "retain" | "remove"`.
- `retire_concept` removes one childless concept and its evidence. Optional
  `replacement` records a semantic successor; it does not move children or
  evidence automatically.

Splits and merges are atomic combinations of these operations. A concept with
children cannot be retired until every child is explicitly moved or retired in
the same proposal.

## Corpus reads and search

```text
annals show [--at REVISION]
annals search QUERY [--limit N]
```

`show` displays HEAD by default. Historical revisions are immutable and
available through `--at`; revision zero is the empty corpus. JSON returns a
revision and a preorder concept array. Each concept contains `path`, `label`,
optional parent path, immediate child labels, and evidence as work-label and
quotation pairs.

`search` queries current concept labels and complete paths. The default limit is
10 and zero is invalid. Results contain paths, labels, and evidence, with no
storage identifiers.

## History

```text
annals log [--limit N]
annals diff FROM TO
annals revert REVISION
```

`log` lists newest commits first; its default limit is 20. Work retention,
proposals, model runs, failed attempts, and no-change examinations are absent
because they are not corpus transitions.

`diff` compares any two retained revision snapshots and reports created,
retired, moved, reordered, and reworded paths plus added or removed quotations.

`revert` inverses one earlier commit against current HEAD and creates a new
commit. It does not erase history. If relevant state has changed since the
target transition, it fails atomically with `revert_conflict`.

## Output and exit behavior

JSON success and failure envelopes are:

```json
{"ok":true,"data":{}}
```

```json
{"ok":false,"error":{"code":"stable_code","message":"description"}}
```

Public corpus JSON uses work labels, path arrays, exact quotations, and revision
numbers. Internal concept, work, proposal, evidence, commit-row, and model-run
identifiers are not exposed.

Exit categories are:

| Code | Meaning |
| ---: | --- |
| 0 | Success |
| 1 | Unexpected process, I/O, or JSON failure |
| 2 | Invalid command or input |
| 3 | Missing library, work, concept, proposal, or revision |
| 4 | Stale state, invariant, review, or reversion conflict |
| 5 | SQLite, integrity, history, or index failure |

Human rendering escapes control characters from retained text and labels.

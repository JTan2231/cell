# Corpus changes and history

Use this feature to submit a strict reconciliation, inspect its proposed
result, apply it, or understand normalization and reversion. Annals owns each
validated transition. A model interpretation and a corpus commit are distinct
records. Read [corpus reading](corpus-explore.md) for graph and evidence meaning
and [libraries](libraries.md) for library selection and instruction revisions.
Use [library operations](library-operate.md) for the operating procedure.

## Interfaces

```text
annals change submit INPUT --work LABEL --base REVISION
annals change list [--limit N]
annals change show [--work LABEL | --at REVISION]
annals change validate [--work LABEL]
annals change apply [--work LABEL]
annals shake [--yes]
annals log [--limit N]
annals diff FROM TO
annals revert REVISION
```

A registered name scopes these commands as `annals library NAME COMMAND`.
Operator config/path selection remains supported. Inspection and validation
are read-only. Submission records intent without changing the corpus.
Application, confirmed normalization, and reversion require authority for their
material effects. No operation here invokes a model.

Rust reconciliation callers use `annals::api::Reconciliation` and
`parse_reconciliation`. `annals::api::CliClient` uses the same typed command
requests and responses as the CLI. Constructing a client has no effects; a
call has the effects of the selected operation. The public interface does not
expose database connections or authorize direct SQL.

## Strict reconciliation input

`change submit` reads JSON from a file or `-`. Its flags provide one retained
evidence work and one frozen base corpus revision. Both are absent from the
semantic request. Direct submission captures the current instruction revision
in its admission transaction.

A request contains a summary, one or more operations, and optional inert
annotations:

```json
{
  "summary": "Integrate predicate locking and phantom prevention",
  "operations": [
    {
      "action": "add_evidence",
      "concept": {"id":"c12"},
      "evidence": [
        {"quote":"A serializable execution has the same effect as some serial execution."}
      ]
    },
    {
      "action": "create_concept",
      "ref": "predicate_locking",
      "label": "Predicate locking",
      "parents": [{"id":"c12"}, {"id":"c27"}],
      "evidence": [
        {
          "quote": "Predicate locks prevent inserts that would change the result of a previously evaluated predicate.",
          "within_heading": ["Transactions", "Avoiding phantom reads"]
        }
      ]
    },
    {
      "action": "add_parent",
      "concept": {"id":"c31"},
      "parent": {"new":"predicate_locking"}
    }
  ],
  "annotations": [
    "The work presents predicate locking as a phantom-prevention technique."
  ]
}
```

Every object rejects unknown fields. Summaries, annotations, labels, handles,
and quotations must be nonempty when present. Labels and handles cannot have
outer whitespace or control characters. `annotations` defaults to an empty
list. Annotations are descriptive context, not evidence, operations, confidence
values, uncertainty flags, or review gates. They do not change the projected
corpus state, validation, mechanical equality, or application decision.

### Concept selectors

An existing concept uses its durable public ID, such as `{"id":"c42"}`.
The spelling is a lowercase `c` followed by a positive canonical decimal
integer. IDs preserve identity through rewording and relationship changes.
Concept labels can repeat and never select a concept.

A same-request creation declares a request-unique `ref`. A selector such as
`{"new":"predicate_locking"}` uses that local handle. A handle can be used
before its creation appears in the request. It has no identity outside that
request. There are no concept-path selectors. Public path arrays locate
headings within source works only.

### Evidence selectors

Evidence belongs to the host-selected work:

```json
{
  "quote": "Exact source language",
  "within_heading": ["Optional", "exact Markdown heading path"],
  "preceded_by": "Optional exact neighboring text",
  "followed_by": "Optional exact neighboring text"
}
```

`quote` is required. The other fields filter its occurrences by exact heading
and immediately adjacent text. A selector selects every remaining occurrence,
subject to bounded fan-out. At least one occurrence must remain. Each selected
occurrence creates a separate exact-range evidence link. Use filters to select
a subset of repeated text. Public input has no source byte offsets.

Evidence supports the concept across all its parent relationships. Every leaf
in the complete projected corpus state needs evidence; non-leaves can have it
too. A retained link covers at most 8 KiB. Source truth and conceptual truth
are not structural validation outcomes.

### Operations

| Action | Meaning |
| --- | --- |
| `create_concept` | Require a request-unique `ref`, `label`, unordered `parents`, and nonempty `evidence`. An empty parent array creates a derived root. Labels can repeat. |
| `add_parent` | Ensure one parent edge exists for `concept`, without changing other parents. An existing edge is an idempotent success. |
| `remove_parent` | Remove one parent edge without relocating the concept or its descendants. Removing the last parent makes the concept a root. |
| `add_evidence` | Ensure links selected by quotations from the scoped work are attached to the selected concept. An existing mapping is an idempotent success. |
| `remove_evidence` | Remove selected quotations from the scoped work that are attached to the selected concept. |
| `reword_concept` | Preserve the public ID and require `evidence_disposition: "retain"` or `"remove"`. |
| `retire_concept` | Remove one concept and its incident edges. Children survive. A child with no remaining parents becomes a root. Optional `replacement` records a successor without transferring edges or evidence. |

Resolution checks reference scope, evidence cardinality, operation ordering,
reword evidence disposition, retirement replacement semantics, and the complete
result. The graph must remain acyclic, with valid endpoints, no self or duplicate
edges, and evidence on every leaf. There is no parent priority, sibling
placement, integer position, concept path, or move operation.

## Submission, selection, and application

Submission resolves and validates a complete projected corpus state but does
not apply it. A material result becomes `pending`. A projected state mechanically
equal to its base becomes `recorded` with no corpus change, no commit, and no
revision advance.

At most one reconciliation per work is pending. A result at the same or a later
base supersedes that work's previous pending result. An older-base result is
retained without displacing a newer pending result. `change list` includes
`pending`, `applied`, `superseded`, and `recorded` records.

`change show --work LABEL` selects the pending reconciliation for that work,
if one exists, otherwise its newest record. Without `--work`, it selects the
sole pending result. With no pending result, it succeeds only if exactly one
work has recorded results. `change validate` and `change apply` select pending
results only. More than one pending work requires `--work`.

Validation reconstructs the request and resolves it at its original base.
Application additionally requires current HEAD and selected instructions to
match the stored corpus and instruction revisions. Both checks occur in the
committing transaction. There is no force path or silent rebase.

One immediate transaction appends a commit and its canonical effects, marks
the reconciliation applied, and completes a linked source-delivery result when
applicable. It updates concepts, parent edges, evidence, history, and revision
atomically. Failure changes neither corpus history nor workflow state.
Committed and recorded results survive later instruction changes and later
Nucleus runtime failures.

Legacy examinations and requests retain unknown instruction provenance as
null. Migration does not invent their instruction basis. They remain
inspectable but cannot supply a current pending interpretation.

## Read proposals and accepted transitions

`change show` returns the full selected reconciliation or accepted transition.
Human output includes public IDs with labels, local creation handles, parent
changes, exact quotations and source context, evidence dispositions,
replacements, and annotations. Validation renders the same semantic facts
without writing.

Resolved evidence gives one item per submitted selector and its
`occurrence_count`. It does not expose resolved byte ranges.
`resolved_operations` describes what the request addressed; `effects` describes
what materially changed. An idempotent ensure can appear in the first without
an effect in the second. Inspection reports the used instruction revision and
the current selection separately.

`change show --at REVISION` reads the commit at that revision. Its effects are
the exact transition from the preceding revision, with the same semantic
entries as `diff PARENT REVISION`. An applied reconciliation also shows its
original request and resolved operations. A revert shows its target and
resolved inverse. A shake shows its reduction request and removed edges.
Each includes actor and timestamp.

Applied and recorded/no-change mutation receipts return work, base/result
revision, status, summary, operation count, and recorded time where applicable.
Pending proposals retain complete review content. `change list` defaults to
20 and returns a schema-two `items`/`has_more` selection page. Increase a
positive `--limit` to read more. Human and JSON modes select the same content.

## Append-only history

Revision zero is the empty corpus. Each applied reconciliation, confirmed
nonempty shake, or successful revert creates one contiguous positive revision.
Work retention, examinations, pending or recorded reconciliations, previews,
and failed attempts do not advance it.

`log` lists commits newest first. `diff FROM TO` replays both selected states
and reports concept creation, retirement, and rewording; individual parent
edges added or removed; and evidence added or removed. It does not invent a
move or reorder event.

Annals reduces current and historical corpus state from append-only typed
concept, parent-edge, and evidence effects. It stores no current-graph snapshot.
Strict replay requires valid transition preconditions and whole-state
invariants. A reducer failure invalidates the library; direct data edits and
skipping effects are unsupported recovery routes.

Annals retains normalized reconciliation intent and provenance. It derives
the resolved request, projected corpus state, canonical effects, and commit
narrative from that intent and replayed states. It does not use stored resolved
JSON as domain authority. Creation reserves durable IDs, even for pending or
superseded requests; those IDs are never reused. A create effect makes the
identity present in the corpus.

## Normalize direct parent edges

`shake` computes transitive reduction at HEAD. It removes a direct parent edge
only when another directed path preserves the same reachability. An
interactive preview gives the base revision, edge counts before and after,
and every proposed removal, then asks once for confirmation.

Only `y` or `yes`, case-insensitively, applies the plan. Another answer or
end-of-file cancels without writing. `--yes` bypasses the prompt. With `--json`
and no `--yes`, the result has `confirmation_required` and exit zero, without
writing. This preview is informational: a later `--yes` invocation computes a
new plan for its then-current HEAD.

Within one invocation, confirmation binds to persistent library identity,
HEAD, the reported graph, and the instruction revision. A stale plan fails
with `shake_stale` and removes nothing. A nonempty confirmed plan appends all
reported edge removals in one transaction and creates one `shake` commit.
No removable edges means no prompt and no revision change.

Normalization preserves concepts, evidence, every ancestor-descendant pair,
roots, leaves, label/ancestor-context search matches and ranking, and
`--within` membership. It does not preserve all paths, direct-neighbor counts,
`shared` flags, hop distances, or revision and direct-relationship metadata in
search output. Instructions can give a direct edge meaning beyond reachability.
Normalization is optional; later reconciliations can add shortcut edges.

## Revert one transition

`revert REVISION` derives the inverse of one earlier commit against current
HEAD and appends the result as a new commit. It never erases the original or
later history. If a relevant concept, edge, or evidence fact changed
incompatibly after the selected transition, the operation fails atomically
with `revert_conflict`. Unrelated facts survive.

## Privacy, bounds, and compatibility

The library retains sources, exact evidence, instruction
revisions, reconciliation and examination provenance, and complete corpus
history. Keep this data private. Reading or validating does not authorize
application, normalization, reversion or disclosure.

A bounded page does not bound replay cost. Revision N replays effects from
1 through N, and the selected corpus state is held in memory. No corpus-size,
throughput, response-latency, or general historical support window is promised.
Feature contract, provider release, library schema, public output schema,
reconciliation grammar, and Nucleus protocol are separate identities.

CLI usage recording requires nonempty `CODEX_THREAD_ID`. Chancery records
command identity, time, and thread ID, not arguments, output, or outcomes.
Internal calls are excluded. Recording errors preserve command results.

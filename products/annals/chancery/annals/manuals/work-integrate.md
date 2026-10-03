# Interpret a source

Use this feature to examine one immutable work against one frozen corpus and
library instruction revision. Annals records the interpretation as semantic
operations with exact source quotations. Library instructions define what the
librarian organizes; Annals enforces source preservation and structural rules.
Read [libraries](libraries.md) for selection and instructions and
[corpus changes](corpus-change.md) for reconciliation input and application.

## Interfaces and effects

```text
annals integrate INPUT [--name LABEL] [--quality QUALITY] [--model MODEL]
  [--apply] [--reexamine]
annals integrate --work LABEL [--quality QUALITY] [--model MODEL]
  [--apply] [--reexamine]
annals change show --work LABEL
annals change validate --work LABEL
```

The first form retains or recognizes source bytes and examines the selected
work. The second examines a retained work. Both deliberately integrate it even
when its bytes were retained earlier. A registered name scopes the command as
`annals library NAME integrate ...`; config/path selection remains supported.
Direct integration requires a `general` library. A `decisions` library permits
examination only from its producer-accepted inbox jobs or explicit retry children.

A compatible authenticated Nucleus connection and initialized prompt selection
are prerequisites. Integration can consume Codex allowance. Reading and
validation do not authorize application. Without `--apply`, a material result
remains pending. Authorized `--apply` commits the validated transition
atomically. A mechanically equal projection is recorded with no corpus change,
no commit, and no revision advance.

## Frozen context and exact reuse

Annals captures HEAD and the selected instructions, checks reuse, and creates
the examination record in one admission transaction. The record freezes the
instruction revision and a hash of the exact effective instructions, pointer
prompt, tool definitions, and result schema. The exact stored library text is
supplied as Nucleus `developerInstructions`. A running examination loads that
recorded revision, including after a new selection becomes current.

Successful reuse and active examination uniqueness require the same work,
base revision, instruction revision, exact effective prompt/tool context,
model, and reasoning effort. Annals can select the newest successful
reconciliation matching that context. `--reexamine` bypasses reuse. Changed
context requires a fresh examination. A → B → A instruction selections append
revisions and do not revive reuse from the first A.

A material proposal can apply only while current HEAD and instructions match
its frozen basis. Annals checks both in the committing transaction. Later
instruction selection starts no reinterpretation and rewrites no committed
history. Recorded and applied results remain authoritative after later
instruction changes or Nucleus failures. Pre-instruction-provenance records
retain null provenance rather than an invented basis.

## Model and requester configuration

`--quality` resolves from the command line, then `[liaison].quality`, then
`high`. The presets select both model and reasoning effort:

| Quality | Model | Reasoning effort |
| --- | --- | --- |
| `low` | `gpt-5.6-luna` | `medium` |
| `medium` | `gpt-5.6-terra` | `medium` |
| `high` | `gpt-5.6-sol` | `max` |

`--model` resolves from the command line, then `[liaison].model`, then the
preset. It changes only the model; quality still chooses reasoning effort.
`[liaison].nucleus_socket` can select a nonstandard local Unix socket. The
default is Nucleus's current-user socket.

Annals submits one closed Nucleus job with its frozen prompt, instructions,
model, reasoning effort, and exact nine-tool contract. Nucleus owns process
isolation, authentication, job and attempt state, and raw protocol output.
Annals does not read or set `CODEX_HOME` and has no direct Codex fallback.
Nucleus's own feature contracts define its scheduler and authentication rules;
a Nucleus terminal outcome does not decide an Annals domain result.

Annals gives current toolset registrations a version derived from the exact
Bazaar selection version plus 2, above its historical registration range.
Input schema IDs are also versioned with the selection because their field
descriptions can change. Historical toolset version 2 and `.input.v2` identities
retain their original text from selection 1. Structural schema changes still
require a code change and the requester compatibility procedure. The result
schema remains `annals.liaison-tool-result.v1`, with historical decoding retained.
The Nucleus job identity is deterministic for the examination. Ambiguous
submission repeats byte-identical request content. Annals caches each tool
result before mailbox transmission, so ambiguous transport retry does not
execute its backend operation twice.

## Scoped reading and draft tools

The liaison is a constrained model role. Its pointer prompt identifies the
work label and frozen base revision and omits complete source text and
repository instructions. Tools supply source and corpus context as needed.
There are no shell, web, planning, user-input, or multi-agent tools.

| Tool | Purpose |
| --- | --- |
| `work_overview` | Read bounded source structure and headings. |
| `work_read` | Read bounded regions through natural source anchors. |
| `work_search` | Search the selected source through bounded queries. |
| `corpus_search` | Search the frozen corpus through bounded queries. |
| `corpus_inspect` | Read bounded corpus concepts, relations, evidence, or local graph context. |
| `submit_reconciliation` | Start a complete reconciliation draft. |
| `revise_reconciliation` | Replace or remove named operations, append operations, or revise metadata. |
| `reconciliation_status` | Recall the compact draft roster or exact staged operations. |
| `discard_reconciliation` | Discard the open draft so a new submission can start. |

Source reads use natural heading, quotation, continuation, or document-edge
anchors, not byte offsets. A heading or quotation anchor must resolve uniquely.
Evidence-selector fan-out is a separate reconciliation rule and does not apply
to source reading. The liaison preserves source material regardless of
estimated novelty or salience and chooses concept granularity relative to the
work, corpus, and stored instructions.

Every tool request crosses strict JSON ingress. Annals validates recognized
shape and bounds and stores normalized intent. Raw arguments and results are
hashed audit artifacts; Annals never decodes those artifacts to resolve,
apply, search, diff, revert, or normalize the corpus.

## Draft lifecycle

A run has at most one open draft. Initial submission creates a normalized
request and stable operation slots. A malformed slot has no action and a repair
hint; malformed raw JSON is not domain state. Annals assesses individual
operations, then resolves the active set as a whole.

A `needs_changes` result preserves independently valid operations. Stable
positive operation IDs such as `op-3` identify draft-local slots. Revision
replaces named slots, explicitly marks removed slots dropped, appends new
slots, or changes metadata. Unmentioned slots remain unchanged. Replacing
another operation never renumbers a slot. An operation ID is distinct from a
concept ID or request-local concept handle.

When every active operation works together, submission or revision finalizes
the draft automatically and records exactly one reconciliation using the same
normalized request. Terminal request and draft rows are sealed. Explicit
discard lets the liaison start again. A run ending without a reconciliation
abandons its open draft. Discarded and abandoned drafts remain audit records;
they create no reconciliation or corpus change.

A draft is neither a pending reconciliation nor a projected corpus state.
The [corpus-change feature](corpus-change.md) owns the strict request grammar,
selectors, seven operations, projected-state validation, and pending/application
semantics.

## Domain results, diagnostics, and inspection

Success means Annals recorded one valid reconciliation or selected an exact
reusable reconciliation. Application success means Annals committed the
resulting state. Final model prose is diagnostic and is never parsed as the
reconciliation. Nucleus completion without the required Annals record is
insufficient. A later runtime failure does not erase a recorded domain result
or authorize a new attempt automatically.

Each examination audit record includes work, base, model, reasoning effort,
prompt version, instruction revision, and context hash. Its domain state is
`running`, `submitted`, `no_submission`, or `failed`; it is not a corpus
revision. Tool calls retain their ordered name, success, time, exact arguments
and result, and content hashes. Final diagnostic response and execution
diagnostics do not form a second durable reporting stream.

`change show --work LABEL` returns the complete selected reconciliation.
`change validate` re-resolves it without writing. Applied and recorded/no-change
receipts return work, base/result revision, status, summary, operation count,
and recorded time where applicable. Pending proposals keep full review
content. `change list` defaults to 20 and uses schema-two `items`/`has_more`;
increase positive `--limit` for more. Annals Usage owns consumption reporting.

## Prompt selection through Bazaar

Prompt preparation requires initialized private Bazaar state and a complete
`cell.prompts.annals` selection. The default database is
`~/.local/share/bazaar/bazaar.sqlite3`; an absolute `CELL_BAZAAR_DATABASE`
override is supported. Reads fail without creating state or using embedded
fallback text. Missing or invalid selections stop new request preparation
before model admission. Deployment supplies no missing prompt contents.

Read `bazaar.prompts.prepare` for the shared selection format, exact component
loading, rendering, and trusted description expansion. Annals chooses its
component set and supplies runtime values. Bazaar prepares the selected text;
Annals assembles its typed request and toolset.

Import the reviewed migration seed before deployment. Preserve selection
version 1 and every referenced text version for historical compatibility.
Runtime reads never import it. Annals resolves the exact components before
new work and records the selection in the examination prompt version and
context digest, freezing resolved content with its request or domain snapshot. Retried retained
requests keep that selection; new examinations follow their documented current
instruction admission. Later text edits do not rewrite saved work.

Authored meaning, runtime inputs, models, permissions, schemas, tool execution,
request assembly, commits, and recovery remain Annals-owned. Library instructions
are immutable domain captures selected through library operations. Stored text
grants no additional authority.

Use `annals.library.operate` for an authorized component or selection edit.
Read `bazaar.prompts.import` before an explicit reviewed import. Keep private
text out of logs and retain historical versions.

## Limits and private state

| Operation or retained value | Limit | Default |
| --- | --- | --- |
| Liaison execution bound | 60 minutes | — |
| App-server transcript | 64 MiB | — |
| Retained model-error tail | 64 KiB | — |
| `work_read` regions per call | 1–20 | — |
| Characters per work region | 12,000 | 4,000 |
| Work overview heading characters | 16,000; truncation reported | — |
| Work or corpus search queries per call | 1–20 nonempty queries | — |
| Work-search matches per query | 1–10 | 5 |
| Work-search excerpt characters | 1,000 | — |
| Corpus-search matches per query | 1–50; separate cursor per query | 10 |
| `corpus_inspect` requests per call | 1–20 | — |
| Parent, child, evidence, or root page | 1–100 items | 25 |
| Relation preview in concept inspection | At most 20 items | 5 |
| Local graph | Depth 0–5; at most 500 concepts; frontier reported | — |
| Model-facing evidence excerpt | 2,000 characters; truncation reported | — |

These bounds do not promise model completion, queue time, throughput, maximum
source size, or allowance consumption. A bounded answer does not bound corpus
replay cost. Selected state is held in memory and historical reads replay
canonical effects.

Complete source text, library instructions, frozen corpus context, tool values,
and diagnostics can enter immutable Nucleus requests and retained operational
state. Protect Annals and Nucleus state as sensitive. Read access does not
authorize source publication or disclosure. Feature version, release, library
schema, prompt/tool identities, model identity, and Nucleus protocol evolve
separately; no general pending-reconciliation support window is promised.

CLI usage recording requires nonempty `CODEX_THREAD_ID`. Chancery records
command identity, time, and thread ID, not arguments, output, or outcomes.
Internal calls are excluded. Recording errors preserve command results.

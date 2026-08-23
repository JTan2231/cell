# Annals vocabulary

This document standardizes how contributors describe Annals and how the
project is discussed in conversational and other nontechnical interfaces. It
provides one conceptual model in two registers: precise technical vocabulary
and plain-language equivalents.

This is contributor guidance, not a corpus ontology or an implemented runtime
contract. It is not input to the Annals liaison and must not be added to the
prompt or context of any model spawned through `model_runner::Runner`, including
tests and future entry points. The executable's isolated prompt and tool schemas
govern the liaison; [Architecture](architecture.md) and [CLI](cli.md) describe
that behavior for repository readers.

## Core model

The ordinary successful model-assisted flow is:

```text
source delivery ---------------------------> delivery record
       |
       | retain new bytes or recognize existing bytes
       v
      work + base revision
                 |
                 v
        examination / model run
                 |
                 v
        reconciliation request
                 |
          resolve and validate
                 |
                 v
       projected corpus state
          /                \
 equal to base        differs from base
       |                    |
       v                    v
recorded; no commit       pending
                            |
                    apply when requested
                            |
                            v
              applied reconciliation + commit
                            |
                            v
                    new revision / HEAD
```

A delivery may fail before it produces a work. Several deliveries may select
the same content-addressed work. `integrate --work` selects an existing work and
does not create another delivery. A reconciliation may also be submitted
without a model run through `change submit`.

A fresh inbox delivery that recognizes already retained bytes stops at the
retention boundary. Its delivery record is completed with `duplicate`
retention and result `retained`, and its job envelope is archived in
`duplicates/`; it produces no examination, reconciliation, or commit. Explicit
manual integration, including an input whose bytes are already retained and
`integrate --work`, continues through the model-assisted flow above.

## Technical vocabulary

### State and source boundaries

**Library**
: One SQLite database managed by Annals, including retained works, delivery
  records, corpus state and history, model runs, and reconciliations. Use
  *database* only when the SQLite implementation matters.

**Corpus**
: The revisioned semantic collection maintained by Annals. It consists of
  concepts, their explicit parent edges, and evidence links. Retained works,
  delivery history, model runs, and reconciliation records belong to the
  library but are not themselves corpus state.

**Corpus state**
: The complete set of concepts, explicit parent edges, and evidence links at
  one revision. Use *projected corpus state* for a complete proposed result and
  *revision snapshot* for its retained historical representation.

**Concept graph**
: The concepts and explicit parent edges in a corpus state. Evidence belongs to
  the corpus state but is not part of graph topology. Avoid using *graph* as a
  synonym for complete state when evidence is included.

**Source delivery**
: One occasion on which source material is supplied through a manual command or
  the filesystem inbox. Each delivery has its own lifecycle and metadata even
  when its bytes select an already retained work.

**Delivery record**
: The durable database record for one source delivery. Existing storage and
  some technical output use *ingestion* or *source-delivery receipt* for this
  record. In ordinary prose, prefer *delivery record* and qualify any use of
  *receipt*.

**Work**
: A named, immutable, retained UTF-8 source object. Exact bytes are
  content-addressed, so several source deliveries can select one work. Reserve
  *work* for this domain object; use *job* or *queue item* for operational work
  waiting to run.

**Retention / ingestion / integration**
: *Retention* stores new immutable bytes or recognizes bytes already retained.
  The `ingested_at` timestamp marks that successful event, and *ingestion* also
  appears in internal storage names. *Integration* attempts to examine a work
  against a base revision, record a reconciliation, and optionally apply it.
  The delivery result `retained` means processing completed at the retention
  boundary, as for `work add` or a fresh duplicate inbox delivery. There is no
  public `ingest` command.

**Work label / source name**
: A work label is the normalized-unique public selector for a retained work. A
  source name is metadata captured for one delivery, usually its incoming
  filename. In prose, prefer *work label* over *work name*. The CLI option
  `--name` supplies a work label.

**Inbox job / job envelope / job receipt**
: An inbox job is one durable FIFO queue item. Its envelope is the filesystem
  directory containing unchanged source material and `job.json`. Call
  `job.json` the *job receipt*. It is a mutable operational record distinct from
  the database delivery record; always qualify which record or receipt is
  meant. Successful integrated jobs are archived in `done/`, fresh duplicate
  jobs in `duplicates/`, and permanently failed jobs in `failed/`.

**Work and delivery times**
: `first_retained_at` belongs to a work and records when those content-addressed
  bytes first entered the library. It remains unchanged across duplicate
  deliveries. `first_seen_at`, `ingested_at`, and `completed_at` belong to one
  delivery. `source_created_at` and `source_modified_at` are captured filesystem
  metadata, not document authorship, publication, or dates found in source text.

### Concept graph

**Concept**
: A corpus-owned semantic identity addressed by a durable public concept ID.
  Rewording or changing relationships does not change its identity.

**Concept ID**
: The canonical public spelling `cN`, such as `c42`, where `N` is a positive
  decimal integer. The ID, not the label or a graph position, selects an
  existing concept.

**Concept label**
: Descriptive text for a concept. Concept labels need not be unique and are
  never selectors. Always qualify *label* when the distinction from a work
  label matters.

**Concept selector / local handle**
: A selector identifies an existing concept by `{"id":"cN"}` or a concept
  created in the same reconciliation by `{"new":"handle"}`. The creation's
  `ref` declares that request-local handle. A handle is not a concept ID or
  label and has no meaning outside its request.

**Parent edge**
: One explicit directed relationship from a broader parent concept to a
  narrower child concept. Parent edges form an unordered directed acyclic
  graph. A concept may have several parents; none is primary. Edges are untyped
  and do not carry evidence.

**Parent / child and ancestor / descendant**
: Parent and child name the endpoints of one direct parent edge. Ancestor and
  descendant describe reachability through one or more parent edges. An
  explicit direct edge may coexist with a longer path that implies the same
  ancestry.

**Root / leaf / shared concept**
: A root concept has no parents, a leaf concept has no children, and a shared
  concept has more than one parent. All three are derived from the explicit
  edge set rather than stored as placements. Every leaf must have evidence.

**Evidence link**
: An association between one concept and an exact quotation from one retained
  work. Annals resolves and stores the quotation's byte range internally.
  Evidence supports the concept across all its parent relationships; it is not
  attached to an edge. Non-leaf concepts may also have evidence.

**Evidence-grounded**
: Subject to the invariant that every derived leaf has at least one evidence
  link. The phrase does not mean that Annals determines whether a concept is
  true or that every concept must carry evidence directly.

### Interpretation and application

**Liaison**
: The constrained model-side role that examines one immutable work against one
  frozen corpus revision through Annals' session-scoped tools. It proposes an
  interpretation but does not own concepts or apply changes.

**Examination / model run**
: An examination is the activity of a liaison reading one work in one frozen
  context. A model run is its durable record, including configuration, status,
  tool-call transcript, and final diagnostic response. It is not a corpus
  revision.

**Base revision**
: The frozen corpus revision against which a reconciliation is constructed and
  resolved. Applying a pending reconciliation requires HEAD still to equal its
  base revision.

**Reconciliation request**
: One strict language-level request containing a summary, one or more semantic
  operations, and optional inert annotations. The host scopes it to a work and
  base revision. It is a provisional, best-current interpretation rather than
  a claim of unique or final semantic decomposition.

**Resolved reconciliation**
: The validated semantic operations, with selectors and quotations resolved,
  together with the complete projected corpus state. Annals compares that state
  mechanically with the base.

**Reconciliation record**
: The durable library record containing the request, resolved reconciliation,
  provenance, status, and any applied revision. A reconciliation request can be
  discussed without implying that it became a commit.

**Projected corpus state**
: The complete corpus state that would result from a resolved reconciliation.
  Prefer the complete phrase rather than an unqualified *projection*, which can
  also name a bounded internal graph selection.

**Operation / resolved operation / effect**
: An operation is one requested semantic action. A resolved operation records
  the durable identities and exact evidence addressed by that action. An effect
  is an actual difference between the base and resulting corpus states. An
  idempotent operation may therefore have no corresponding effect.

**Annotation**
: Inert free-form context retained with a reconciliation. An annotation is not
  evidence, an operation, a confidence level, an uncertainty flag, or a review
  gate. Its shape and nonempty text are contract-validated, but it does not
  affect projected corpus state, mechanical equality, graph validation, or the
  application decision.

**Apply**
: Atomically accept a pending reconciliation's projected corpus state, append a
  commit, and advance the revision. Submission and validation alone do not
  apply a reconciliation.

**Recorded / no corpus change**
: The reconciliation status `recorded` means its projected corpus state is
  mechanically equal to its base. The interpretation is retained, but it
  creates no commit and advances no revision. Outside machine-value discussion,
  prefer *recorded with no corpus change* or *no-change interpretation*.

### History

**Revision**
: A public address for one immutable corpus state. Revision zero is the empty
  corpus. Each commit advances the library by exactly one positive revision;
  retaining a work or merely recording a reconciliation does not.

**Commit**
: One accepted corpus state transition. Commits form a linear, append-only log
  and are keyed by the revision they produce. Applied reconciliations, confirmed
  nonempty shakes, and reverts create commits.

**HEAD**
: The current materialized corpus revision and state.

**Snapshot**
: A complete representation of corpus state containing concepts, explicit
  parent edges, and evidence links. A snapshot may represent HEAD, a projected
  result, or retained history. A *revision snapshot* is the immutable historical
  form stored for an addressable revision. Neither is a bounded graph view.

**Shake**
: A confirmed transitive reduction of HEAD. It removes explicit parent edges
  already implied by longer directed paths while preserving every
  ancestor-descendant pair. A confirmed nonempty shake creates a commit; an
  empty or cancelled shake does not.

**Revert**
: Apply the inverse of one earlier commit to current HEAD and append the result
  as a new commit. Reversion does not erase or rewrite the earlier commit.

### Reads

**View / graph projection**
: A bounded read result for a selected revision, such as a concept detail,
  page, or local graph neighborhood, is an output view. Internal graph code may
  call its selected subset a graph projection. Neither is complete corpus state
  or a projected corpus state.

**Cursor**
: An opaque continuation token for one paged request. It is bound to the same
  library, resolved revision, and request scope and carries no conceptual
  meaning.

**Frontier**
: The reported boundary where a local graph view stopped because of depth or
  node limits. A frontier entry is not necessarily a leaf concept.

**Lexical search**
: Word-based matching over normalized concept labels and derived ancestor-label
  context. It is not semantic similarity, evidence retrieval, or a truth claim.

## Lifecycle namespaces

Several independent lifecycles reuse words such as `applied`, `recorded`, and
`failed`. Always qualify a status or result with its owner in technical prose.

| Scope | Field | Machine values |
| --- | --- | --- |
| Model run | status | `running`, `submitted`, `no_submission`, `failed` |
| Reconciliation record | status | `pending`, `applied`, `superseded`, `recorded` |
| Source delivery | status | `processing`, `completed`, `failed` |
| Source delivery | channel | `manual`, `inbox` |
| Source delivery | retention | `new`, `duplicate` |
| Source delivery | result | `retained`, `pending`, `applied`, `recorded` |
| Inbox job receipt | state | `processing`, `done`, `failed` |
| Inbox job receipt | `result_status` | `retained`, `applied`, `recorded` |
| Commit | kind | `change`, `shake`, `revert` |
| Shake invocation | status | `unchanged`, `confirmation_required`, `cancelled`, `applied` |
| Recent activity | `time_basis` | `created`, `modified`, `first-seen`, `ingested`, `completed` |

Within `incoming/`, *ready* and *settling* classify files; they are not job
receipt states. Keep the exact filesystem and database words distinct: a *done
job receipt* and envelope correspond to a *completed delivery record*. *Stale*
is an application or plan failure caused by changed state, not a stored
reconciliation status. A completed source delivery has one result; processing
and failed deliveries do not. Retention remains independent and may be present
even when later processing fails. A done job receipt has one `result_status`.
The `duplicates/` directory is an archive category, not another receipt state:
its receipts have state `done` and result status `retained`. Historical
envelopes keep their existing archive and result.

## Plain-language register

Use this register when introducing Annals, writing conversational interfaces,
or discussing it with someone who does not need its storage and protocol
details. These expressions simplify the language without changing the model.

| Technical term | Preferred plain-language expression |
| --- | --- |
| Annals library | knowledge library |
| work | retained source document |
| source delivery | source arrival or import attempt |
| delivery record | source history entry |
| corpus | evidence-backed map of ideas |
| concept | idea |
| concept label | idea label or idea name |
| parent edge | broader/narrower link |
| root concept | idea with no broader parent |
| leaf concept | idea with no narrower child |
| shared concept | idea linked under several broader ideas |
| evidence link | exact supporting quotation |
| liaison | AI reader |
| examination or model run | reading pass or examination pass |
| reconciliation request | proposed interpretation or proposed update |
| projected corpus state | preview of the updated idea map |
| pending reconciliation | proposed update waiting to be accepted |
| recorded reconciliation | examined source whose interpretation produced no map change |
| apply | accept and save the proposed update |
| commit | saved update |
| revision | saved version |
| HEAD | current version |
| shake | simplify redundant links |
| revert | undo an earlier update while preserving history |

### Approved descriptions

Short description:

> Annals is a local, versioned knowledge library that keeps source documents
> unchanged and organizes their ideas into an evolving map backed by exact
> quotations.

Conversational description:

> Give Annals a source document and it can use an AI reader to compare that
> document with the current idea map, propose how its ideas fit, and preserve
> the supporting quotations. Accepted proposals become numbered versions;
> readings that produce no change are still recorded.

Do not simplify Annals into a document summarizer, folder hierarchy, or system
that decides truth. Its organization is provisional, revisable, multi-parent,
and grounded in exact source language.

## Usage rules

- Use *library* for all stored state and *corpus* for revisioned semantic state.
- Reserve *work* for the immutable retained source object; use *job* or *queue
  item* for operational work waiting to run.
- Qualify *label* as a work label or concept label when ambiguity is possible.
- Qualify *root* as a root concept or spool root when ambiguity is possible.
- Say *reconciliation request*, *reconciliation record*, *operation*, *effect*,
  or *commit* instead of using *change* for all of them.
- Qualify *receipt* or avoid it: use *delivery record* for database history and
  *job receipt* for `job.json`.
- Call the current concept structure a directed acyclic graph, not a tree.
- Use *concept* as the domain noun. *Node* is appropriate only for a graph-view
  representation such as the public `nodes` array.
- A source document may have a heading path. A concept has no canonical path,
  placement, primary parent, sibling position, or move operation.
- Describe search as lexical or word-based. Do not imply semantic similarity.
- Do not claim that Annals judges truth or constructs a uniquely correct or
  final conceptual decomposition.

## Historical vocabulary

The preserved experiment archive documents earlier Annals contracts. When
*tree*, *path*, *placement*, *move*, or *node* describes Annals' corpus or its
topology there, it is historical language rather than part of the current
model. The same words may still appear legitimately in concept labels or when
describing a source's subject matter.

The archive also uses *proposal*, *outcome*, and *uncertainty* as formal names
from pre-reconciliation contracts. They are not one-to-one aliases for current
reconciliation concepts, and annotations are inert notes rather than renamed
uncertainties or application gates. It may also name the obsolete operations
`move_concept` and `attach_evidence`. Current reclassification uses explicit
`add_parent` and `remove_parent` operations, evidence uses `add_evidence`, and
there is no move operation. Preserve the historical walkthrough wording
verbatim. When referencing it from current work, identify the language as
historical. Ordinary phrases such as *proposed update* remain appropriate in
plain language. See the [experiment preservation
policy](../experiments/README.md#historical-runners).

## Ownership

This document owns project term definitions and the mapping between technical
and plain-language registers. The CLI, architecture, data-model, search, and
installation documents own behavioral contracts and may explain terms locally
when needed, but should not assign them conflicting meanings.

New public concepts, lifecycle fields, or conversational names should be added
here when introduced. Internal Rust type and SQLite table names do not need
glossary entries unless they surface in the CLI, JSON, liaison tools, persisted
machine values, or project discussion.

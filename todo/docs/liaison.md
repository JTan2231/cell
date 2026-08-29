# Research liaisons

Todo uses three narrowly different model stages. They share a runtime but not
a prompt, toolset, or authority:

| Stage | Input question | Durable result | Cannot do |
| --- | --- | --- | --- |
| Concern routing | Which durable concern identity, if any, matches `cN`? | one pending `rN` | apply or authorize routing |
| Situation assessment | What is true now, and who owns each state or decision? | one immutable `aN` | revise direction or choose design |
| Design reconciliation | What desired state satisfies the direction against `aN`? | one `dN` draft or durable assessment return | plan, implement, or accept design |

Model text is never authority. The validated Todo tool result is the domain
result, and user decisions occur later through deterministic CLI commands.

## Common runtime boundary

Each stage submits one closed Nucleus job with an immutable stage-specific
toolset and result schema. Todo services the durable tool mailbox until the job
is terminal. Nucleus owns job state, raw protocol, Codex compatibility,
authentication, and its single-use launch context. Todo owns every domain
record created through a validated tool call.

The version-2 stages have no shell, filesystem workspace, inherited process
environment, or web search. They can inspect only the frozen material exposed
through their stage tools or prompt. This makes the evidence boundary a
durable part of the record rather than whatever happened to be visible to a
broad research process.

Source, candidate, and evidence text is untrusted input, not runtime
instruction. Every material routing, assessment, and design claim carries
explicit basis references. Infrastructure failure is reported as failure; a
liaison cannot turn missing tools or unavailable evidence into a domain fact.

The legacy `research-liaison` toolset remains immutable so historical Nucleus
jobs can be decoded. New public `todo new` invocations do not use its
model-authorized `create_todo` behavior.

## Concern routing

The host has already captured `cN` before this stage begins. It freezes the
concern source and admitted ancestry plus a bounded snapshot of plausible todo
umbrellas, including exact direction revisions. The liaison can page, read, or
search the bounded source material; page through candidate summaries; inspect
one candidate in detail; and submit one proposal.

The routing prompt requires one of:

- attach to one unchanged `tN`;
- create a new `tN` with a proposed title, direction, and complete boundary
  set;
- revise one `tN` whose enduring identity remains the same;
- unify exactly two `tN` identities, naming the survivor and a complete
  reconciled direction;
- dismiss when positive evidence establishes that no action remains; or
- defer when the evidence or a material user choice is insufficient.

The liaison must preserve the user's direction and distinguish explicit user
statements from assistant proposals and its own inference. Similar words,
directory proximity, age, or a shared source are not identity evidence.

Managed tools:

```text
routing_source_overview
routing_source_read
routing_source_search
routing_candidates
routing_candidate_inspect
submit_concern_routing
```

`submit_concern_routing` validates IDs, direction revisions, action-specific
fields, evidence references, and limitations. It records one pending `rN`.
There is no accept, reject, create-todo, revise, or unify tool in the session.

`todo new DIRECTION --source PATH` first uses deterministic concern capture and
then invokes this stage. `todo concern assess cN` invokes only this stage for an
existing concern.

## Situation assessment

The host supplies one established umbrella, its current direction and explicit
boundaries, the attached-concern and note bases, any accepted design, and a
frozen catalog of relevant sources. The liaison may page through that catalog,
read a named source, search within a named source, and submit one assessment.
Concrete document entries have stable `s-...` source IDs. Reads and searches
return exact `source:<source-id>@...` evidence references; when the assessment
commits, Todo persists each used document under the corresponding
`source:<source-id>` `source_ref`, with its locator, frozen revision, and
observation time. Submitted source evidence must resolve through that persisted
mapping. The host's frozen Todo projection is persisted separately as the
`todo-snapshot` base.

Each assessed jurisdiction names all relevant parties, assigns each exactly one
role of `owner`, `participant`, or `consumer`, describes each responsibility,
and has exactly one owner.

The prompt requires the assessor to distinguish committed, pushed, deployed,
configured, in-progress, reverted, and merely proposed work. It maps every
direction boundary to observed findings and records jurisdiction rather than
assuming Todo owns external state.

Managed tools:

```text
situation_sources
situation_source_read
situation_source_search
submit_situation_assessment
```

An assessment contains a summary, subject identity, grounded findings,
jurisdiction findings, direction mappings, unresolved items, and one
disposition:

- `ready`: evidence is adequate for design reconciliation;
- `needs_user_choice`: a material value or authority decision cannot be
  inferred; or
- `inconclusive`: a material evidence gap remains.

The assessor cannot alter the todo, route a concern, propose desired
architecture, or turn a liaison runtime or tool failure into an inconclusive
assessment.

## Design reconciliation

The host resolves `design propose tN` to one exact current ready `aN` and
supplies that assessment, the current direction boundaries, and any accepted
prior design. There are no external research tools in this stage because new
facts belong in a new assessment.

The host also supplies a closed catalog. The admitted grammar is exactly:

```text
direction:body
direction:<local_ref>
assessment:<aN>
assessment:<aN>:finding:<local_ref>
assessment:<aN>:jurisdiction:<key>
design:<dN>:<op-N>
correction:<agent_job_id>
```

The direction, assessment, predecessor-operation, and correction entries are
generated only from the exact records bound to this run. A liaison cannot
invent an alias or cite another assessment, predecessor, or correction.

The liaison proposes desired ownership and boundaries through named,
basis-linked records:

- jurisdiction changes use `keep`, `move`, `add`, or `retire` with exact
  expected and proposed multi-party assignments: `keep` preserves the owner,
  `move` changes it, `add` has no expected set, and `retire` has no proposed
  set; every nonempty set has exactly one owner and may retain participants and
  consumers, and a jurisdiction that continues is represented explicitly with
  `keep`;
- clauses cover ownership, boundary, state, interface, lifecycle, failure,
  compatibility, acceptance, and non-goals; and
- unresolved choices identify material questions the supplied user direction
  cannot decide.

A design is not a work plan. The prompt forbids implementation tasks, file
edits, commands, sequencing, estimates, deployment actions, and execution
steps. A ready or accepted design is therefore not an instruction to execute
and not evidence that implementation happened.

Managed tools:

```text
submit_design_reconciliation
revise_design_reconciliation
design_reconciliation_status
discard_design_reconciliation
return_for_assessment
```

The first submission is atomic. The host validates the complete jurisdiction
map, clauses, choices, references, and assembled design before allocating a
`dN`; if any part is invalid, it records none of the submission and the liaison
must correct and resubmit the complete draft. A successful initial submission
creates one `dN` and assigns stable operation IDs. It remains `open` while
active choices exist; a complete zero-choice submission can seal as `ready` in
the same transaction.

An open draft seals as `ready` only after it has no active unresolved choices,
contains active clauses of all nine kinds (`ownership`, `boundary`, `state`,
`interface`, `lifecycle`, `failure`, `compatibility`, `acceptance`, and
`non_goal`), and collectively cites `direction:body`, every structured
direction boundary, and every active operation in its exact predecessor.
Basis references are checked on initial submission, every revision, and this
final readiness transition.

After that point, revision uses an expected draft version and explicitly
replaces, adds, or drops only named operations. Omitted operations remain. The
liaison may discard an irreparable draft or return to assessment with an exact
reason and ordered missing-or-stale references.

Dropping a staged jurisdiction-change operation during revision removes that
proposal operation. It is not the same as the `retire` action, which proposes
that a jurisdiction cease to exist in the desired state.

`return_for_assessment` is a separate durable terminal outcome for the design
run, not a `dN` state. Todo records the design-stage job, exact `aN`, reason,
producer tool call, time, and ordered `missing_or_stale_refs`. A return before
initial submission allocates no `dN`. If an open draft exists, the same
transaction marks it `abandoned` and links it from the return. A ready or
otherwise terminal design cannot be returned. Once recorded, the return ends
that liaison run and later submit or revise calls cannot change its outcome.

If the job instead ends with a still-open draft and no assessment return, Todo
atomically marks the draft `abandoned` with a concise reason. It remains
inspectable and can be corrected, but it is never treated as ready merely
because the Nucleus job stopped.

`design correct dN FEEDBACK` uses the named draft and exact caller feedback as
additional immutable basis. Before model work, Todo inserts one immutable
`todo_design_corrections` record keyed by the new design-reconciliation job,
binding that job to the predecessor, feedback, creation time, and its
`correction:<agent_job_id>` basis reference. Exact replay is idempotent; the
record cannot be edited. Correction is permitted from `ready`, `rejected`, or
`abandoned` when the exact bound assessment remains current and `ready`, never
from an active `open` draft. The named predecessor is not edited; a successful
run allocates a successor `dN`. It can produce a corrected ready proposal, but
cannot accept it. There is no design-accept or design-reject model tool.

## Explicit decisions

`routing accept`, `routing reject`, `design accept`, and `design reject` bypass
Nucleus entirely. Each requires `--source PATH`, and each stores the resolved
path as authorization provenance. Rejections also require a retained reason.
The source file must exist and be readable UTF-8 at decision time, but Todo
stores no source contents.

The accept commands recheck their recorded bases and authorize in one Todo
database transaction; rejection records a reason without applying the proposed
change. A model cannot invoke either path, and final model prose, a completed
Nucleus job, or a ready draft is never treated as implicit authorization.
Routing acceptance requires every referenced umbrella to remain open and
canonical. Design acceptance likewise requires the owning `tN` to remain open
and canonical and the bound `aN` to remain current. Because any newer
assessment makes an older `aN` stale, a design against that older assessment
must be reconciled again rather than authorized or corrected.

## Model selection

`concern assess`, `assess`, `design propose`, `design correct`, and the research
part of `new` accept `--quality low|medium|high` and `--model MODEL`.
`--quality` selects the configured reasoning preset and defaults to `high`;
`--model` overrides only that preset's model. The same defaults may be supplied
under `[liaison]` in Todo configuration.

# Architecture

Todo has one synchronous executable and one local SQLite database. It owns no
daemon, network service, usage sidecar, workflow engine, or background queue.
The optional macOS schedule is a launchd timer that invokes the same executable
for email; it is not a resident Todo process. Model execution is delegated to
the separately installed, per-user Nucleus service.

Todo's architecture follows its statement types:

```text
provenance        identity                  observed state        desired state

cN concern --rN decision--> tN umbrella --assessment--> aN --reconciliation--> dN
 immutable         explicit     revisioned      dated       proposed / accepted
```

The arrows do not mean automatic progression. A concern and an umbrella remain
useful without an assessment; an assessment can remain inconclusive; a design
can remain proposed or rejected. Plans, work items, implementation execution,
and deployment are outside this model. Nucleus execution of a research liaison
is runtime provenance for producing one Todo record, not execution of the
accepted design.

## Ownership boundaries

Todo SQLite is authoritative for:

- concern provenance and terminal concern disposition;
- routing proposals, their frozen bases, and explicit decisions;
- stable todo identities, direction revisions, attachments, unifications,
  status, and working notes;
- dated situation assessments and the bases against which they were made;
- design drafts, clause revisions, decisions, and assessment bindings; and
- requester-job correlation and the exact accepted domain result.

Nucleus is authoritative for job admission, runtime state, authentication,
Codex compatibility, tool-mailbox delivery, and raw protocol history. It does
not own `cN`, `rN`, `tN`, `aN`, or `dN`, and Nucleus completion is not Todo
success. Git, component documentation, deployed binaries, and other external
systems remain authoritative for facts in their own jurisdictions; an
assessment cites those authorities rather than copying ownership into Todo.

Conversation or file paths are provenance references. Todo retains paths and
evidence references, not source bytes. Codex may read source material during a
bounded research job, and Nucleus's retained raw JSONL can therefore contain
source or derived content. Treat the Nucleus state directory as part of the
same local security and retention boundary.

## Concern capture and routing

Capture is deterministic and happens before research. Todo resolves the source
to an absolute readable UTF-8 regular file and inserts one immutable concern.
If later Nucleus submission or model work fails, the `cN` remains available for
retry through `concern assess cN`.

Routing runs against a frozen candidate snapshot. The liaison can inspect only
the bounded candidate records exposed by Todo and can submit exactly one
pending action: attach, create, revise, unify, dismiss, or defer. The managed
tool records the model's proposal and evidence; it cannot change a todo
identity or decide its own proposal.

`routing accept` is a separate deterministic transaction. The caller supplies
a source file for authorization provenance. Before applying anything, Todo
rechecks the concern state, proposal state, candidate identities, direction
revisions, and other recorded basis cursors. Every referenced umbrella must
still be open and canonical. A stale comparison aborts the whole transaction.
A successful transaction records the decision and applies exactly the named
action:

- attachment adds provenance without changing the umbrella direction;
- creation establishes a new umbrella and its first direction revision;
- revision appends a direction revision to the same enduring identity;
- unification chooses the proposal's canonical survivor and preserves every
  superseded identifier and its history;
- dismissal records that the concern has no retained actionable outcome; and
- deferral preserves the unresolved concern and the reason it could not yet be
  routed.

Rejection records its reason and decision provenance without applying the
action. A shared source or similar title is never enough to turn attachment
into revision or unification.

`todo new` composes capture and routing research only. It never calls the
authorization transaction.

## Situation assessment

Assessment is a descriptive read-and-record operation on one `tN`. The host
freezes the todo's current direction revision, attached concerns, note cursor,
accepted design, and available evidence sources. The liaison maps every
direction boundary to grounded findings. Each jurisdiction describes one state
or authority concern through assignments of parties as `owner`, `participant`,
or `consumer`, with exactly one owner and a concrete responsibility for every
assigned party.

The frozen catalog gives each concrete document a stable `s-...` ID. Bounded
reads emit location-bearing evidence references under
`source:<source-id>@...`; for every document used, the committed assessment
persists the matching
`source:<source-id>` base with its locator, revision, and observation time.
This makes the citation-to-source mapping part of the immutable `aN`, rather
than relying on a later reconstruction of the research run. The frozen Todo
projection has its own persisted `todo-snapshot` base.

The managed assessment tool can commit one immutable `aN` with a disposition
of ready, needs-user-choice, or inconclusive. It cannot alter the umbrella or
choose a future design. Infrastructure failure produces no domain assessment;
it is not converted into an “inconclusive” claim.

An assessment records when it was observed and the exact inputs it used. Later
changes do not mutate that historical record. Reads compare those inputs with
the umbrella's current projection and report concrete stale reasons. Any newer
`aN` for that umbrella makes every older assessment non-current. “Accepted then
stale” and “never accepted” therefore remain distinguishable.

## Design reconciliation

`design propose tN` first resolves the latest current ready `aN` for that
umbrella, then rechecks that exact choice before starting the liaison. The
design job receives the immutable direction and assessment basis plus any
accepted prior design. It explicitly classifies jurisdiction changes as
`keep`, `move`, `add`, or `retire`, preserving full expected and proposed
multi-party assignments where applicable. `keep` preserves the owner, `move`
changes it, `add` has no expected assignments, and `retire` has no proposed
assignments. Every nonempty assignment set has exactly one owner; participants
and consumers remain explicit rather than being collapsed into ownership.

The job also produces named clauses with explicit basis references. Ownership
and boundary clauses cite an assessed jurisdiction. A design describes desired
ownership, boundaries, state, interfaces, lifecycle and failure semantics,
compatibility, acceptance properties, and non-goals; it is not an
implementation plan, and accepting it does not perform implementation.

The host supplies a closed basis catalog whose exact forms are
`direction:body`, `direction:<local_ref>`, `assessment:<aN>`,
`assessment:<aN>:finding:<local_ref>`,
`assessment:<aN>:jurisdiction:<key>`, `design:<dN>:<op-N>`, and
`correction:<agent_job_id>`. Direction entries come only from the bound
revision, assessment entries only from the bound `aN`, design entries only
from active operations in the exact predecessor, and the correction entry only
from the current correction job. Every submitted or revised operation is
checked against this catalog.

Draft construction is recoverable within one liaison turn. Independently valid
parts are not staged piecemeal on the first submission: Todo validates the
complete initial design atomically, and an invalid submission creates no `dN`.
A successful submission creates a `dN` with stable operation IDs. It remains
open while active choices exist; a complete zero-choice submission can seal as
ready in that same transaction. Later revision can replace, add, or explicitly
drop named operations; omission never silently deletes one.
`design correct dN FEEDBACK` starts a new bounded run from the named
predecessor. Before the run starts, Todo writes the exact feedback and
predecessor into immutable `todo_design_corrections`, whose
`correction:<agent_job_id>` reference becomes an admitted basis. The named
`dN` is not mutated; success allocates a successor `dN`. Correction is allowed
from `ready`, `rejected`, or `abandoned` when its assessment remains current
and `ready`, never from the still-active open draft of another liaison run.

If the bound assessment cannot support a design, `return_for_assessment`
records a separate immutable terminal outcome tied to the design job and exact
`aN`, with a reason and ordered missing-or-stale references. It is not a design
state and need not allocate a `dN`. If the run already has an open draft, the
same transaction abandons and links that draft; a ready or terminal draft
cannot be returned.

If the liaison ends while its draft is still open and has not recorded an
assessment return, Todo atomically marks the draft `abandoned` with a concise
reason. The draft remains inspectable and correctable, but it is not ready and
cannot be authorized.

Ready means structurally ready for a user decision, not accepted. It requires
no active choices, active coverage of all nine clause kinds (`ownership`,
`boundary`, `state`, `interface`, `lifecycle`, `failure`, `compatibility`,
`acceptance`, and `non_goal`), and collective basis coverage of the direction
body, every direction boundary, and every active predecessor operation.
Acceptance and rejection are deterministic commands unavailable to the model,
and both retain a canonical decision-source path. Acceptance rechecks the
draft, its exact `aN`, the current direction and attachment basis, and competing
decisions in one transaction. It additionally requires the owning umbrella to
be open and canonical; a newer assessment makes the bound `aN` stale. Rejection
records a terminal reason without applying the desired state. There is no force
path around a stale authorization basis.

## Runtime and atomicity

Each research operation submits one closed, versioned Nucleus invocation with
the tools for only that stage. Registrations are immutable, so changed tool or
result meaning requires a new toolset/schema version. Nucleus owns the
single-use launch context and Codex authentication; Todo neither reads nor
copies Codex credentials.

Todo services the durable tool-call mailbox and polls the durable Nucleus job
and attempt state until the job is terminal. It does not infer execution state
or diagnostics from log records. A completed liaison's final prose comes from
the structured attempt output; failure detail comes from the attempt's terminal
message. Nucleus harness stderr is not part of Todo's retained or live
diagnostic surface.

A validated tool call is committed in Todo before its result is returned to
Nucleus. That domain commit can outlive a requester disconnect, daemon restart,
or later job failure. Replayed calls use stable job, call, and operation
identities so the same call returns the same result rather than duplicating a
concern, proposal, assessment, or draft operation. The committed domain record,
not final model prose, is the result.

Authorization commands never run through the model mailbox. They open Todo's
database directly and use immediate transactions, so stale-basis validation,
the decision, and the authorized state change either all commit or none do.

SQLite uses foreign keys, a busy timeout, WAL journaling, read-only connections
for reads, and immediate transactions for writes. Migration is separately
explicit and backup-bearing; ordinary opens never migrate implicitly.

## Read projections

The normalized history remains append-only, but the CLI presents a compact
current view:

- `concern show cN` combines provenance, proposal history, and terminal
  disposition;
- `routing show rN` combines the sealed action, bases, and decision;
- `show tN` combines the latest direction, concerns, status, notes, latest
  assessment, design state, supersession, and stale reasons;
- `situation show aN` preserves the dated facts and shows whether their bases
  still match; and
- `design show dN` combines clauses, corrections, decision provenance, and
  staleness.

Staleness is derived comparison data, not another mutable status vocabulary.
The underlying accepted or rejected decision remains historical truth.

## Daily attention digest

The email commands are deterministic read projections and do not invoke a
liaison. The projection includes unresolved captured concerns and every open
canonical todo, with enough derived assessment and design state to identify
what kind of attention is next. Completed and superseded todos remain
excluded. The projection is rendered as the same subject and content in plain
text and HTML; no digest or delivery state is added to SQLite.

The body groups items under **Needs your decision**, **Needs follow-up**, and
**Other open todos**. A current title or plain-language label comes first,
followed by a plain-language status, secondary typed references, and safe
inspection commands. Stored state tokens are translated rather than exposed.
The decision section covers routing proposals, situation choices, and
desired-state designs awaiting an explicit user decision. Follow-up covers
unresolved concerns without a pending routing decision and open todos that need
assessment, reassessment, more evidence, or desired-state design work. Other
open todos have no immediate
research or decision request; an accepted desired state does not imply that
implementation ran or that the todo is complete.

The digest discloses only aggregate counts, current todo titles, generic
plain-language stage labels, typed references, and inspection commands. It
excludes concern bodies, directions, notes, source paths, assessment and design
summaries, unresolved-choice text, and evidence. Assessment and design state
is used to classify entries without copying that richer content into email.

`email preview` makes no external call. `email send` posts immediately to
Resend using the existing retry and idempotency contract. Email sending does
not use Nucleus or Codex authentication. The rendered digest leaves the local
security boundary in both the email and Resend's service.

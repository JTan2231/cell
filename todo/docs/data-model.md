# Data model

SQLite schema version 2 stores the domain as typed relational records. There
are no JSON or JSONB columns. Structured assessments and designs are normalized
so IDs, references, decisions, and bases can be validated with ordinary keys
and constraints.

## Public identities

| Spelling | Record | Identity meaning |
| --- | --- | --- |
| `cN` | concern | one immutable originating direction and source |
| `rN` | routing proposal | one sealed proposed disposition for a concern |
| `tN` | todo umbrella | one enduring actionable concern across revisions |
| `aN` | situation assessment | one immutable observation against frozen bases |
| `dN` | design | one basis-bound design draft and decision history |
| `nN` | working note | one append-only note on an umbrella |

Prefixes are part of the public type. `c3` and `t3` are unrelated even when
their storage row numbers happen to match.

## Concerns

A concern contains:

- the caller's nonblank direction;
- the canonical absolute path of the readable UTF-8 file where it arose;
- creation time; and
- its current routing disposition.

The direction, path, and creation time are immutable. Todo stores no source
bytes, digest, transcript format, or source-retrieval cache. A concern normally
starts unresolved. An accepted attachment or creation associates it with a
todo; dismissal is terminal. Deferral records why routing cannot yet be
decided without inventing an umbrella identity.

One concern attaches directly to at most one umbrella. Several concerns may
attach to the same `tN`. After unification, historical links remain physically
on their original umbrellas; the survivor's read projection derives inherited
effective concerns through the supersession relationship. It does not rewrite
one `cN` onto several `tN` rows.

## Todo umbrellas and direction revisions

A todo umbrella contains its stable `tN`, `open` or `done` lifecycle state,
creation/completion timestamps, and an optional canonical successor after
unification. The title and direction shown by the CLI come from the latest
direction revision.

Every direction revision is append-only and contains:

- a revision number unique within the umbrella;
- a nonblank single-line title;
- a nonblank direction body;
- the complete set of explicit boundaries, each with stable local reference
  and basis references; and
- the routing decision or legacy migration that established it.

Revision changes meaning while preserving identity. Unification is different:
it explicitly selects one of two umbrellas as the survivor, retains both
identifiers and all their provenance, and prevents the superseded identity from
becoming a second current result.

`done` and `reopen` remain idempotent umbrella transitions. They do not create
an execution record or prove that a particular design was implemented.

## Routing proposals and decisions

A routing proposal belongs to exactly one concern and records:

- disposition: attach, create, revise, unify, dismiss, or defer;
- exact target `tN` values and direction revision numbers;
- proposed title, direction, and boundaries when the action needs them;
- the selected survivor and reconciled direction for a two-umbrella
  unification;
- rationale, evidence references, limitations, and frozen basis cursors;
- model job/call provenance; and
- pending, accepted, rejected, or invalidated decision state.

The proposed content is sealed before a decision. Acceptance rechecks every
basis in the same immediate transaction that records the decision and applies
the action; every referenced target must still be an open canonical umbrella.
Rejection records a required reason without applying it. Both decisions retain
the canonical path of the authorizing source; optional conversation thread or
turn metadata remains null unless Todo can establish it deterministically.

A newer proposal does not delete an older one. Invalidated or stale proposals
remain readable as history.

## Situation assessments

An assessment binds one `aN` to:

- one `tN` and exact direction revision;
- the captured attachment set and working-note cursor;
- the accepted-design basis, if one existed;
- a bounded source catalog with stable source IDs and model job/call
  provenance; and
- its observation and creation times.

Its normalized content includes a summary, subject identity references, typed
findings, jurisdiction findings, direction-to-finding mappings, and unresolved
items. A jurisdiction has a stable key, the state or authority concern it
governs, evidence, and one or more party assignments. Each party appears at
most once and has a concrete responsibility under role `owner`, `participant`,
or `consumer`; every jurisdiction has exactly one owner. The assessment
disposition is `ready`, `needs_user_choice`, or `inconclusive`.

Every admitted document source has a stable `s-...` ID. Each document actually
used by an assessment is persisted in `todo_assessment_bases` under a unique
`source_ref` of the form `source:<source-id>`, together with its kind, locator,
frozen revision, and observation time. Evidence returned by bounded reads uses
that same prefix plus an exact location, such as
`source:<source-id>@line:<line>` or
`source:<source-id>@chars:<start>-<end>`. Todo rejects assessment evidence
whose source prefix has no persisted base, so the historical `aN` retains the
mapping from a citation to the frozen source it denotes. The frozen Todo
projection is persisted separately with `source_ref` `todo-snapshot`.

Assessments are immutable descriptions. A later concern attachment, note,
direction revision, accepted design, or authoritative external-state change
does not edit the row. The read projection compares stored bases to current
state and reports stale reasons. The existence of a newer `aN` for the same
umbrella is itself a stale reason for every older assessment, even if its other
bases still match: only the newest observation can be current. Staleness is
derived; it is not a replacement for the original disposition.

## Designs

A design belongs to one `tN` and binds the exact `aN` used for reconciliation.
It retains a summary, versioned draft operations, current assembled content,
model job/call provenance, and decision history.

Design content is normalized into:

- jurisdiction changes with action `keep`, `move`, `add`, or `retire`, exact
  expected and proposed assignment sets, rationale, and basis references;
- named clauses of kind `ownership`, `boundary`, `state`, `interface`,
  `lifecycle`, `failure`, `compatibility`, `acceptance`, or `non_goal`;
- basis references on every jurisdiction change, clause, and unresolved
  choice; and
- unresolved material choices.

Each nonempty jurisdiction assignment set has exactly one owner and may also
name participants and consumers. `keep` requires the same expected and
proposed owner, while `move` requires a different owner. `add` requires an
empty expected set and `retire` an empty proposed set. Ownership and boundary
clauses must cite an assessed jurisdiction. Continuing an assessed jurisdiction
is represented explicitly by `keep`, not by omission.

Each design run receives one closed basis catalog. These are the only admitted
reference forms:

- `direction:body`;
- `direction:<local_ref>` for a boundary in the bound direction revision;
- `assessment:<aN>`;
- `assessment:<aN>:finding:<local_ref>`;
- `assessment:<aN>:jurisdiction:<key>`;
- `design:<dN>:<op-N>` for an active operation in the exact predecessor; and
- `correction:<agent_job_id>` for the exact correction run.

There are no shorthand or free-form basis references. Todo validates every
active operation against that catalog on creation and revision and revalidates
the assembled design before it becomes ready.

`design correct` first records the caller's exact nonblank feedback in the
immutable `todo_design_corrections` table. The row is keyed by the new
design-reconciliation agent job and binds that job to its exact predecessor
`dN`, derived `correction:<agent_job_id>` basis reference, and creation time.
Exact replay is idempotent; different feedback cannot overwrite the row. A
correction may use a `ready`, `rejected`, or `abandoned` design as predecessor,
but never a draft that is still `open` in its original liaison run, and its
bound assessment must still be current and `ready`. The predecessor row is not
mutated; a successful correction run allocates a successor `dN`.

Initial design creation is atomic: the complete jurisdiction map, clauses,
choices, references, and assembled result validate before Todo allocates a
`dN`. Draft operations then have stable operation IDs and expected versions.
Replacement, addition, and explicit drop are distinct, so omitted content
cannot disappear silently during correction or retry. A draft can become
`ready` only when it has no active unresolved choices, has at least one active
clause of each of the nine kinds, and its active operations collectively cite
`direction:body`, every structured direction boundary, and every active
operation in its predecessor design. If a liaison run ends with a still-open
draft and no assessment return, Todo atomically marks that draft `abandoned`
with a concise reason. It remains inspectable and may be the predecessor of
`design correct`. A draft may also be explicitly discarded without becoming
accepted. Dropping a staged jurisdiction-change operation removes that draft
operation; it does not mean `retire`, which is a normative desired-state
action.

Acceptance and rejection retain a canonical decision-source path; rejection
also retains a nonblank reason. Acceptance succeeds only while the exact
assessment, direction, attachment, draft-version, and competing-decision bases
remain current and the owning `tN` is still open and canonical. A newer `aN`
therefore prevents authorization of a design bound to the older assessment.
Later changes can make an accepted design's basis stale, but do not erase the
historical acceptance. Acceptance adds no plan, work item, implementation
execution record, or implementation-completion evidence.

## Assessment returns from design

An assessment return is an immutable outcome of one design-reconciliation job,
separate from design identity and state. It stores the exact `aN`, reason,
producer tool-call identity, creation time, and ordered structured
missing-or-stale references. It optionally links an abandoned `dN`.

Returning before the first valid submission creates no design. Returning after
an open draft atomically marks that draft `abandoned` and links it to the return.
A ready or terminal design cannot be returned. The return is terminal for the
liaison run, so retries resolve to the same outcome rather than later creating
a draft from that run.

## Working notes

Each `nN` contains a required parent `tN`, nonblank text, and creation time.
Working notes are append-only. They cannot be updated or deleted, and reads
order them by creation time then ID. The latest note ID is also a stable cursor
for assessment-basis comparison.

## Agent correlation and idempotency

Todo stores the domain stage, requester/job identity, tool-call identity, and
exact result binding needed to replay a managed call safely. Nucleus owns the
raw protocol and runtime state; Todo does not copy those logs into its domain
schema. A committed result can therefore be returned again after reconnect
without creating a second domain row.

## Version-1 migration

Migration is explicit and preserves legacy meaning conservatively:

1. Every legacy `tN` keeps its public ID, `open`/`done` state, and timestamps.
2. Its title and pointer become direction revision 1, marked as legacy-derived.
3. Its pointer and source path produce one captured concern attached to that
   same umbrella.
4. Its original researched note is retained byte-for-byte as a
   `legacy_unreviewed` design. It is not accepted and is not silently split
   into an assessment, accepted design, or implementation plan.
5. Existing working notes are preserved in order and receive `nN` identities.
6. Migration infers no relationship or unification between legacy todos and no
   implementation or closure evidence from a `done` marker.

`todo migrate --backup ABSOLUTE_PATH` requires an absent absolute backup path
for a version-1 database. It writes a complete SQLite backup before applying
the version-2 transaction. Failure leaves the original usable; success retains
the caller's backup. Running the command against an already-current database is
a true no-op and does not touch the supplied backup path.

Constraints and triggers prevent mutation of immutable provenance, sealed
proposals, assessments, decided designs, and existing working notes. Foreign
keys use restrictive semantics; Todo does not expose deletion commands.

## Email projection

Email configuration, API credentials, rendered digests, Resend identifiers,
send attempts, and delivery status are not stored in SQLite. A digest is a
current read projection over open canonical umbrellas. The installed
configuration owns the sender and recipient, the process environment owns
`RESEND_API_KEY`, and Resend owns its external delivery and short-lived
idempotency records.

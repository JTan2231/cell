# CLI contract

## Command surface

```text
todo [--config PATH] [--database PATH] [--json] [--quiet] [-v...] COMMAND

todo init
todo migrate --backup ABSOLUTE_PATH

todo concern add DIRECTION --source PATH
todo concern list [--all] [--limit N]
todo concern show cN
todo concern assess cN [--quality low|medium|high] [--model MODEL]

todo new DIRECTION --source PATH [--quality low|medium|high] [--model MODEL]

todo routing show rN
todo routing accept rN --source PATH
todo routing reject rN --reason TEXT|- --source PATH

todo list [--all] [--limit N]
todo search QUERY [--all] [--limit N]
todo show tN
todo note add tN TEXT|-
todo done tN
todo reopen tN

todo assess tN [--quality low|medium|high] [--model MODEL]
todo situation show aN

todo design propose tN [--quality low|medium|high] [--model MODEL]
todo design show dN
todo design correct dN FEEDBACK|- [--quality low|medium|high] [--model MODEL]
todo design accept dN --source PATH
todo design reject dN --reason TEXT|- --source PATH

todo email preview
todo email send [--scheduled]
```

Global options work before or after the subcommand. Commands are
noninteractive. A documented `-` text argument reads one UTF-8 value from
standard input.

Public IDs identify distinct durable concepts: `cN` is a concern, `rN` a
routing proposal, `tN` a todo umbrella, `aN` a situation assessment, `dN` a
design, and `nN` a working note. IDs are not interchangeable.

## Concern intake and routing

`concern add` validates and resolves `--source`, then commits one concern with
the caller's direction and the absolute source path. It performs no model work
and makes no identity decision. The source must be an existing readable UTF-8
regular file. Todo retains its path, not its bytes.

`concern list` returns unresolved concerns newest first. `--all` also includes
concerns already attached to an umbrella or dismissed. Results are bounded by
`--limit`.

`concern assess cN` researches one captured concern against a frozen, bounded
snapshot of candidate umbrellas and records one pending `rN`. The proposal has
one action:

- `attach`: associate the concern with an existing unchanged umbrella;
- `create`: create a new umbrella for an enduring concern;
- `revise`: append a direction revision to an existing umbrella without
  changing its identity;
- `unify`: preserve several historical identities while selecting one
  canonical umbrella;
- `dismiss`: retain the concern but record that positive evidence leaves no
  actionable outcome;
- `defer`: retain the unresolved choice because evidence or user direction is
  insufficient.

Research never performs the action. `routing show rN` displays the proposed
action, exact frozen bases, rationale, evidence, limitations, and decision
state. `routing accept` applies a still-pending, still-current proposal in one
Todo transaction. `routing reject` records a terminal negative decision and
requires a nonblank reason. Both commands require `--source PATH`; Todo
canonicalizes and validates that readable UTF-8 file and retains it as the
authorization provenance. The decision source is not reopened or copied.

An accepted attachment does not revise an umbrella. A revision does not merge
identities. A unification preserves every source concern, revision, note, and
prior identifier while marking the noncanonical umbrellas as superseded.
Neither lexical similarity nor a common source permits automatic unification.

If the concern, candidate set, direction revision, proposal state, or another
record in the proposal's frozen basis changed, or any referenced umbrella is
no longer open and canonical, acceptance fails with a stale basis conflict.
There is no `--force`; reassess the concern instead.

`todo new` is exactly the convenience boundary “capture, then research a
pending routing proposal.” The concern commit happens before model submission
and survives model or Nucleus failure. `new` never accepts the resulting `rN`,
creates or revises a `tN`, or treats final model prose as authorization.

## Todo umbrella reads and lifecycle

A `tN` is the stable identity for one enduring actionable concern. Its current
title and direction are projections of the latest direction revision, not
mutable columns that erase history. `show tN` reports the current revision,
attached concerns, notes, latest assessment and its stale reasons, proposed
and accepted design state, and any supersession relationship.

`list` and `search` return open canonical umbrellas by default. `--all`
includes completed or superseded history where applicable. Results use a
stable deterministic order and are bounded by `--limit`. Search matches the
umbrella read model rather than exposing revision tables as separate results.

`note add` appends one immutable `nN` working note. `done` and `reopen` remain
idempotent status transitions on the umbrella. They do not imply that a
particular implementation plan ran, and no new execution or closure-evidence
model is introduced by this version.

## Situation assessment

`assess tN` maps the umbrella's accepted direction boundaries to current
observed state. It freezes the current direction revision, attached-concern
set, note cursor, accepted design (if any), and the evidence sources made
available to the assessor. The result is one immutable, dated `aN` with:

- subject identity and stable identity references;
- grounded current-state, constraint, dependency, and gap findings;
- jurisdiction findings that assign each system or actor a concrete
  `owner`, `participant`, or `consumer` responsibility, with exactly one owner
  per jurisdiction;
- a mapping from every direction boundary to those findings;
- material user choices, evidence gaps, or jurisdiction conflicts; and
- a disposition of `ready`, `needs_user_choice`, or `inconclusive`.

The source catalog gives each concrete document a stable `s-...` ID. Reads
return citations under `source:<source-id>@...`; the committed `aN` persists
the matching `source:<source-id>` `source_ref` with the source locator,
revision, and observation time. `situation show aN` renders that mapping
alongside the findings and their evidence. The frozen Todo projection is a
separate persisted base named `todo-snapshot`.

An assessment is descriptive. It cannot revise the todo, choose a desired
architecture, or authorize anything. `situation show aN` always shows the
historical record and its exact bases. When those bases no longer match the
umbrella, the read model reports the assessment as stale; it does not rewrite
or revoke the historical assessment. Creating any newer `aN` for the same
umbrella also makes every older assessment non-current.

## Design reconciliation

`design propose tN` resolves the umbrella's latest current `ready` assessment
and rechecks it before submission. The resulting `dN` stores that exact `aN`
as its basis; the caller never supplies an `aN` to this command and Todo never
silently switches assessments after the run begins.

Every operation cites one or more entries from the exact basis catalog for the
run. Its complete admitted grammar is:

```text
direction:body
direction:<local_ref>
assessment:<aN>
assessment:<aN>:finding:<local_ref>
assessment:<aN>:jurisdiction:<key>
design:<dN>:<op-N>
correction:<agent_job_id>
```

The host derives those entries only from the bound direction, assessment,
active operations of the exact predecessor, and—during correction—the exact
stored feedback. Free-form references and references to other records are
rejected.

The design classifies every jurisdiction transition it addresses as `keep`,
`move`, `add`, or `retire`. `keep` preserves the assessed owner while allowing
an explicit responsibility-set change; `move` changes the owner; `add` starts
without expected assignments; and `retire` ends without proposed assignments.
Every nonempty expected or proposed set assigns each party once as `owner`,
`participant`, or `consumer` and contains exactly one owner. Ownership and
boundary clauses name their supporting assessed jurisdiction. A jurisdiction
that continues is represented explicitly with `keep`; omission is not another
change action.

The remaining clauses cover desired ownership, boundaries, state, interfaces,
lifecycle and failure semantics, compatibility, acceptance properties, and
explicit non-goals. They do not contain implementation tasks, file edits,
commands, sequencing, estimates, deployment actions, or implementation
execution records.

The first design submission is all-or-nothing. Todo validates its complete
jurisdiction map, clauses, choices, and references before allocating `dN`; an
invalid submission leaves no partial draft. Once an open draft exists, later
revisions name stable operations and use an expected draft version.

No active choices is necessary but not sufficient for `ready`. The draft must
also contain active clauses of all nine kinds (`ownership`, `boundary`,
`state`, `interface`, `lifecycle`, `failure`, `compatibility`, `acceptance`,
and `non_goal`), and its active operations must collectively cite the
direction body, every structured direction boundary, and every active
predecessor operation.

When the exact `aN` is insufficient or stale, the liaison can produce a durable
assessment return instead of a design. The return records the design-stage job,
`aN`, reason, producer call, time, and ordered missing-or-stale references. It
is terminal for that run and is not a `dN` state. No `dN` is allocated when the
return precedes initial submission; if an open draft exists, Todo atomically
marks it `abandoned` and links it to the return. The command reports
`assessment_research_needed` so the umbrella can be reassessed rather than
treating the return as a failed or accepted design.

If a design liaison run ends with an open draft but no assessment return, Todo
atomically marks the draft `abandoned` with a concise reason. It remains
visible through `design show` and is eligible for an explicit correction; it
never becomes ready because the job happened to end.

`design correct dN FEEDBACK` sends the exact named draft plus the feedback to
the design liaison. Todo first stores the exact feedback and predecessor in an
immutable `todo_design_corrections` row keyed by the new liaison job; its
`correction:<agent_job_id>` value is the feedback's admitted basis reference.
Correction is allowed from `ready`, `rejected`, or `abandoned`, never from a
draft still `open` in its original run, and the predecessor's exact assessment
must remain current and `ready`. The named `dN` is not edited; a successful run
allocates a successor `dN`. The liaison may replace, add, or explicitly drop
named operations and seals a corrected proposal; omission is not deletion.
Correction does not accept the result. Dropping a staged
jurisdiction-change operation during correction removes that proposal
operation; it is distinct from a `retire` action, which proposes that the
jurisdiction not exist in the desired state. `design show dN` displays every
operation and basis, the assessment and predecessor bindings, correction
feedback, status, decision provenance, and stale reasons.

`design accept` and `design reject` are deterministic Todo writes, never model
tools. Both require a canonicalized readable UTF-8 `--source PATH`; rejection
also requires a nonblank `--reason`. Acceptance fails if the draft is not
ready, its umbrella is completed or superseded rather than open and canonical,
its assessment is no longer current, its direction or attachment basis
changed, or another design decision won the race. A newer `aN` makes the bound
assessment non-current and therefore blocks both acceptance and correction.
An accepted design remains historically accepted if later facts change, but
the read model marks the basis stale. A new proposal must then reconcile from
a fresh assessment.

Acceptance stops at normative desired state: it does not create a plan, work
item, implementation job, or completion claim.

## Database initialization and migration

`init` creates a current empty database and refuses to replace any existing
path.

`migrate --backup ABSOLUTE_PATH` is the only supported schema-upgrade entry
point. When the selected database is version 1, the backup path must be
absolute and absent. Todo writes and retains a complete SQLite backup there,
then performs the version-2 migration transactionally. It never overwrites a
backup. When the database is already version 2, the command is a no-op and
does not create, inspect, or otherwise touch the backup path.

Migration preserves every `tN`, status transition, source, and working note.
It derives an initial direction revision and captured concern from each legacy
todo, and retains the old researched note as a `legacy_unreviewed` design. That
design is not accepted, and migration does not infer cross-todo identity,
relationships, assessment facts, or completion evidence.

The installed deployer supplies a transaction-local backup path, runs
migration before its smoke test, and restores both the prior database and
release state if a later update step fails. A manually selected backup remains
caller-owned retained recovery material.

## Daily attention digest

`email preview` renders the exact current digest without reading
`RESEND_API_KEY` or making a network request. Human output contains `From`,
`To`, and `Subject` headers followed by the plain-text body. JSON data contains
`from`, `to`, `attention_count`, `pending_concern_count`, `todo_count`,
`subject`, `text`, and `html`. `todo_count` remains the number of open canonical
todos. `pending_concern_count` counts unresolved captured concerns, and
`attention_count` counts the items in **Needs your decision** and **Needs
follow-up**.

`email send` sends that digest immediately through Resend. It requires a
nonblank `RESEND_API_KEY` with no surrounding whitespace in the process
environment and uses `todo-email/<UUIDv7>` as its idempotency key. `email send
--scheduled` changes only the key to
`todo-daily-email/<LOCAL YYYY-MM-DD>`, identifying the most recent local 09:00
occurrence. Neither mode submits a Resend `scheduled_at` value. One invocation
freezes the body and key for up to three total attempts on transport failures,
`429`, or `5xx` responses.

The digest contains unresolved captured concerns and all open canonical todos.
Its body uses **Needs your decision**, **Needs follow-up**, and **Other open
todos**, omitting empty sections. Each entry leads with a current title or
plain-language label, followed by a plain-language status. A `Reference:` line
uses descriptive typed references such as `Todo tN`, `Situation assessment
aN`, and `Desired-state design dN`; one or more `Inspect:` lines give read-only
CLI commands. Raw stored state tokens are not part of the email interface.

The subject reports the attention and open-todo counts with singular-aware
wording, for example `Todo daily: 3 need attention · 6 open todos`. With no
open todos or unresolved captured concerns it is `Todo daily: all clear`, and
the body says that nothing needs attention. The body summary also reports the
unresolved-concern count. Email commands require `[email]` configuration;
other commands do not.

## Database and configuration selection

The development binary requires one database target. `--config PATH` selects a
config before `TODO_CONFIG`; then `--database PATH` selects a database before
`TODO_DATABASE`, which selects one before the configured database. A database
option may override the database from a simultaneously selected config while
retaining that config's liaison settings. A relative database path in a
configuration file is resolved relative to that file. Todo never silently
creates `./todo.db`.

A minimal strict configuration is:

```toml
database = "todo.db"

[liaison]
quality = "high"
# model = "an-exact-model-override"
```

Email delivery is optional:

```toml
[email]
from = "todo@joeytan.dev"
to = "j.tan2231@gmail.com"
```

The sender domain must be verified in Resend. The API key is environment-only;
it is not a supported TOML field. The deprecated `liaison.codex` key remains
parseable during the deployment rollback window but is ignored; there is no
direct-Codex fallback. Nucleus uses `NUCLEUS_SOCKET` when set and its standard
per-user socket otherwise.

## Output and errors

Human output is concise and terminal control characters from database, source,
or model text are escaped before rendering. `--quiet` suppresses successful
mutation acknowledgements but not query results or errors.

`--json` emits exactly one JSON response document: success goes to standard
output and failure goes to standard error. JSON is a CLI protocol; it is not
stored in SQLite. Successes have `ok: true` and a `data` value; failures have
`ok: false` and a stable error object.

A successful model tool call is a Todo domain commit, not a claim that the
Nucleus job ended cleanly. If a concern, proposal, assessment, or draft commits
and a later terminal liaison error occurs, Todo reports the durable result and
may also report the later diagnostic on standard error. Nucleus completion by
itself is never domain success.

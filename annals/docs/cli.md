# Commands and records

- [Select a library](#named-libraries) and [set global options](#global-options).
- [Retain a work](#immutable-works) or [integrate it](#model-assisted-integration).
- [Read source activity](#recent-source-activity) and [browse the corpus](#local-corpus-browsing).
- [Apply a reconciliation](#reconciliations-and-corpus-changes) or [read history](#history).
- [Operate the inbox](inbox.md) or [report usage](telemetry.md).

## Named libraries

```text
annals library list
annals library create NAME [--kind general|decisions]
annals library NAME show
annals library NAME COMMAND
annals library NAME instructions show
annals library NAME instructions history [--limit N] [--before REVISION]
annals library NAME instructions set CONTENT
annals library NAME instructions set --file PATH
annals library NAME instructions set --stdin
```

The catalog maps each name to a stable library ID and its managed database,
config, and spool. Unknown names fail. An ordinary command never creates a
library. `create` initializes revision zero and default instructions before
publishing the library as ready; repeating an interrupted creation resumes
that named operation. Repeating a completed creation with the same kind returns
the same library; conflicting kind or state fails without rebinding.
Creation starts no model job and creates no schedule.

Names contain 1–64 lowercase ASCII letters, digits, underscores, or hyphens,
start with a lowercase letter, and cannot be `list`, `create`, or `help`.
`list` reports registered provisioning state. `show` verifies the database and
returns the registration, corpus revision, and current instruction revision.
Neither operation proves live model or scheduler readiness.

Named scope selects ordinary work, graph, reconciliation, and inbox operations.
It rejects a competing `--library` and ignores `ANNALS_LIBRARY`. Its own config
is the default; an explicit `--config` may supply execution settings but cannot
change the selected database, spool, identity, or admission kind. The resolved
database must match its catalog identity. Config `expected_library_id` and
`--expected-library-id` can also pin an operator path and must agree. Library kind remains an immutable admission boundary:
Conatus or career libraries use `general`; selecting a name or changing its
instructions cannot open generic admission to a `decisions` library.

The catalog lives at `ANNALS_STATE_DIR/catalog.db`. With no override, macOS
uses `~/Library/Application Support/Annals`; other platforms use
`~/.local/share/annals`. Newly created library data lives under
`libraries/LIBRARY_ID/` with `annals.db`, `config.toml`, and `spool/`. Existing
operator-selected paths remain supported and are not automatically cataloged.

`instructions set` requires exactly one nonblank UTF-8 input and preserves its
complete text. It atomically appends and selects one instruction revision.
Identical current bytes return `changed: false`; A → B → A creates three
successive revisions. The result contains `changed` and an `instructions` object with `library_id`,
`revision`, `content`, `sha256`, and `recorded_at`. The timestamp records instruction selection, not
source creation, examination completion, or graph reinterpretation.

`instructions show` reads the current exact instructions. History identifies
`library_id` and `current_instruction_revision` and returns `instructions` and
`has_more`, newest first, with a default limit of 20 and a maximum of 100.
Continue with `--before` the oldest returned revision. Instruction changes
start no examination, change no corpus revision, and leave committed history
intact. Future examinations use the selection captured when they start.

## Global options

```text
annals [--config PATH] [--library PATH] [--expected-library-id ID] [--json] [--quiet] [-v...] COMMAND
```

The config path resolves from `--config`, then a nonempty `ANNALS_CONFIG`.
The library path resolves from `--library`, then a nonempty `ANNALS_LIBRARY`,
then the selected config's `library`. If neither a library nor a usable config
selects one, the command fails with `library_not_configured`. Annals never
defaults to `./annals.db`.

The installed macOS frontend selects
`$HOME/Library/Application Support/Annals/config.toml` only when the invocation
has no explicit config or library selection. Thus bare installed commands such
as `annals stats` use the user installation. Explicit selections such as these
target independent libraries. The literal `annals` invocation with no
subcommand still displays help.

```text
annals --config ./project.toml stats
annals --library ./scratch.db init
ANNALS_CONFIG=./project.toml annals stats
ANNALS_LIBRARY=./scratch.db annals stats
```

An explicit library suppresses the frontend's user-config default. The
uninstalled executable has no implicit config path, so repository and Linux
uses must provide a config or library unless their own launcher supplies one.
Relative `library` and `inbox.root` config paths resolve from the config file's
directory. Command-line and environment paths resolve from the process working
directory.
`--json` emits one success object on stdout or one error object on stderr.
`--quiet` suppresses successful human mutation messages. `-v` prints the
resolved library path on stderr in human mode.

Decision-account acceptance and feed reads are intentionally stricter. They
require an explicit decisions config or a registered name whose managed config
pins the same decisions identity. The operator-path form requires `--config`,
rejects `--library`, ignores `ANNALS_LIBRARY`, and uses only that file's
`library`, `inbox.root`, and `decision_feed.expected_library_id`. Missing or
mismatched identities fail closed rather than selecting the primary Annals
library.

Every version-6 database also carries one immutable `general` or `decisions`
library kind. Decision acceptance, feed reads, and a decision-config run
require `decisions`; generic source-producing commands and a generic inbox run
require `general`. Configuration, spool selection, or a direct `--library`
override cannot reclassify or bypass that database identity.

When `[decision_feed]` is configured, `work add`, direct `integrate`, `inbox
enqueue`, `inbox register`, and the hidden backlog importer fail with
`decision_feed_accept_required`. `inbox run` binds and verifies the dedicated
spool identity, dispatches only producer-accepted originals or their explicit
retry children, and leaves files in `incoming/` unregistered. A generic config
also cannot admit to or run a spool that already carries the decision-library
binding.

The primary inbox keeps its existing behavior.

## Deployment maintenance

```text
annals --library DATABASE --json maintenance status
annals --library DATABASE --json maintenance hold RUN_ID
annals --library DATABASE --json maintenance release RUN_ID
```

The usual explicit config or library selection chooses the maintenance boundary.
Its private durable gate is the sibling directory
`<canonical-database>.cell-maintenance`. Each library has its own gate. These
commands do not open, initialize, or migrate the database. Status leaves an absent
gate absent. The standard success envelope contains `protocol_version: 1`,
`contract_version: 1`, all `holds`, and `drained`. Drain reports whether live
participating commands have released admission; deployment must separately
account for durable unfinished work and dependency jobs.

`hold` atomically prevents new mutating commands while already admitted work
settles. Repeating the same hold is idempotent; holds survive process exit.
`release` removes only the named hold and is idempotent when it is absent.
Run IDs contain 1–128 ASCII letters, digits, hyphens, underscores, or periods
and cannot start with a period. Invalid IDs and unprovable gate state fail
with `deployment_maintenance`.

Public CLI clients use the same boundary. Writes such as `init`, `migrate`,
`work add`, account acceptance, and dispatch fail before database access when
held. Read-only corpus and decision-feed commands remain available. Inbox
pause and interrupt remain available; hold and release never clear operator
pause, and ordinary `inbox resume` is fenced.

An installer may set `CELL_DEPLOYMENT_RUN_ID=RUN_ID` for a controlled command.
When holds exist, this requires exactly that sole owner and exclusive access
after existing activity drains. Another owner or active command prevents it.
With no hold, the command follows ordinary admission. This environment value
does not resume schedules or create blanket bypass authority. Annals Usage
doctor may accept an intentionally held Nucleus only after its typed
deployment-health interface proves the same sole owner, runtime drain,
authentication, and harness readiness; public Nucleus admission stays held.

## Library operations

```text
annals init [--kind general|decisions]
annals migrate
annals stats
annals backup OUTPUT
```

`init` creates revision zero and returns the persistent `library_id` and
immutable kind. It refuses to replace an existing library. The default kind
is `general`. Only the decisions provisioner or an explicitly selected
dedicated-library setup uses `--kind decisions`.

`migrate` upgrades a version-3, version-4, or version-5 library to version 6.
Earlier steps assign version-3 and version-4 libraries the `general` kind and
add retry provenance and the decision-account feed. Version 6 preserves an
existing kind, seeds the default library instructions at instruction revision
1, and adds examination and reconciliation instruction provenance.

Older records keep unknown instruction provenance as null; the seed is not
attributed to their historical examinations. Migration changes no retained
source or corpus history. It rejects schemas older than 3 or newer than this
executable. On version 6 it is an idempotent current-format check. Migration
uses one transaction, so a failure retains the prior version without partial
tables.

A pre-version-3 replacement still requires the guarded `--fresh-state`
cutover. `stats` reports revision and corpus, graph, work, reconciliation,
history, model-run, and database-size information.

`backup` makes a consistent SQLite copy and refuses to replace its destination.

## Krisis decision-account exchange

See [the account-exchange contract](../chancery/annals/manuals/decision-account-exchange.md)
for acceptance, library identity, input validation, and bounded feed reads.

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
labels and sizes; JSON also reports SHA-256 digests and `first_retained_at`.
Human `work show` labels that timestamp `First retained`. It is the time Annals
first retained those content-addressed bytes, not the source file's creation or
modification time. `work show` also reports Markdown heading paths and the
complete unchanged text. Source heading paths describe the document; they are
not concept paths.

## Model-assisted integration

```text
annals integrate INPUT [--name LABEL] [--quality QUALITY] [--model MODEL] [--apply] [--reexamine]
annals integrate --work LABEL [--quality QUALITY] [--model MODEL] [--apply] [--reexamine]
```

The first form retains or recognizes and examines the selected work. The
second examines an already retained work. Both are explicit manual integration
and retain this behavior when the bytes were supplied before. Annals freezes
the current corpus and instruction revisions in one admission transaction,
invokes the liaison with those exact stored instructions, and expects one
recorded reconciliation.

The liaison starts a complete draft with `submit_reconciliation`. If Annals
reports `needs_changes`, independently valid operations remain staged while
`revise_reconciliation` changes only named operation IDs.
`reconciliation_status` recalls the compact roster or exact stored operations,
and `discard_reconciliation` abandons the complete draft so a fresh submission
can start. Submission or revision records automatically when every active
operation works together.

The model's final response is diagnostic and is not parsed as the
reconciliation.

Annals may reuse the newest successful reconciliation for the exact same work,
base revision, instruction revision, exact effective prompt/tool context, model,
and reasoning effort. `--reexamine` bypasses this lookup. A later corpus or
instruction revision, or changed liaison configuration, starts a fresh
examination. Returning to earlier instruction bytes does not revive old reuse.
A pending material proposal can apply only while both HEAD and the instruction
selection still match its frozen basis, checked in the committing transaction.
Completed results survive later instruction changes and runtime failure.

`--quality` accepts three presets. Its value resolves from the command line,
then `[liaison].quality` in the selected config, then `high`:

| Quality | Model | Reasoning effort |
| --- | --- | --- |
| `low` | `gpt-5.6-luna` | `medium` |
| `medium` | `gpt-5.6-terra` | `medium` |
| `high` | `gpt-5.6-sol` | `max` |

`--model` resolves from the command line, then `[liaison].model`, then the
model selected by the quality preset. It changes only the model; the selected
quality continues to choose reasoning effort. `[liaison].nucleus_socket`
optionally selects a nonstandard Nucleus Unix socket. Annals submits the same
prompt, base and developer instructions, model, reasoning effort, and exact
nine-tool contract as one Nucleus job. There is no direct Codex fallback.

Nucleus owns the isolated app-server process, persistent authentication,
canonical credential refresh, and eight-slot scheduling. Annals asks Nucleus
for an authenticated account preflight before a queued dispatch; it may wait
up to 30 seconds, and failure leaves the envelope queued with attempts zero.

The liaison submits an interpretation of the retained work at the frozen base
revision. It preserves source material regardless of estimated novelty or
salience and chooses concept granularity relative to the work and corpus.

Without `--apply`, a reconciliation whose projected corpus state differs from
its base remains pending. `--apply` immediately commits that pending
transition. A projected corpus state mechanically equal to the base is stored
with status `recorded`; it creates no commit and does not advance the revision.
Optional annotations are inert and never block application.

## Consumption telemetry

See [usage reporting](telemetry.md) for `annals-usage report`, `budget`,
`doctor`, and `login`, including accounting and coverage rules.

## Scheduled inbox

See [inbox operations](inbox.md) for admission, dispatch, priority, pause,
interruption, bounded retry, status fields, and crash recovery.

## Recent source activity

```text
annals lately [--since TIME] [--until TIME]
              [--by created|modified|first-seen|ingested|completed]
              [--status processing|completed|failed]
              [--channel manual|inbox]
```

`lately` reports source-delivery metadata, independently of the source's text
or interpreted concepts. It never searches source content or emits source
text, headings, quotations, topics, or dates mentioned inside a work.

The report uses a UTC half-open interval: `since` is inclusive and `until` is
exclusive. `--until` defaults to the instant at which the report begins.
`--since` defaults to `7d`. A relative `--since` is subtracted from the
resolved `until`, so an explicit end and relative start produce a reproducible
window. Relative durations are a positive integer followed by `s`, `m`, `h`,
`d`, or `w`.

Absolute values are either an RFC 3339 timestamp or a `YYYY-MM-DD` UTC date,
interpreted as midnight at the start of that date. RFC 3339 offsets are
accepted and normalized to UTC in output. The start must precede the end.

Examples:

```text
annals lately
annals lately --since 24h
annals lately --since 2026-08-01 --until 2026-08-15
annals lately --since 7d --by modified
annals lately --since 30d --status failed --by completed
annals lately --since 7d --channel inbox
```

`--by` chooses the timestamp used for both inclusion and newest-first ordering:

| Basis | Meaning |
| --- | --- |
| `created` | Filesystem creation time captured when the source arrived, when supplied by the operating system. |
| `modified` | Filesystem modification time captured when the source arrived. |
| `first-seen` | When Annals first observed the delivery. |
| `ingested` | When Annals retained the bytes as a new work or recognized them as an existing work. This is the default. |
| `completed` | When delivery processing reached the completed or failed state. |

Created and modified times are captured source metadata. They do not represent
authorship, publication, events described by the source, or continued watching
of the original path. Standard input has neither filesystem timestamp. A
failure before work retention has no ingestion time, and an active delivery
has no completion time.

`--status` accepts `processing`, `completed`, or `failed`. `--channel` accepts
`manual` or `inbox`. The manual channel covers a source passed to `work add` or
the input form of `integrate`; `integrate --work LABEL` selects an existing work
and is not another delivery. Filters are applied before the time window.

Every delivery has its own receipt. Delivering identical bytes again creates a
second receipt linked to the original immutable work. Retention is therefore
reported independently as `new` or `duplicate`. Lifecycle status is also
independent of the terminal result:

| Result | Meaning |
| --- | --- |
| `retained` | `work add` or a fresh duplicate inbox delivery completed at the retention boundary. |
| `pending` | Integration completed with a reconciliation awaiting application. |
| `applied` | Integration completed and created the reported corpus revision. |
| `recorded` | Integration completed without a corpus change. |

A retry child likewise appears as a new inbox source delivery, while its
original remains failed at its original completion time. `lately` reports each
delivery independently; use `inbox retry status` for their event and
parent-child relationship.

A processing delivery has not reached a terminal outcome and has no result. An
inbox job-processing error fails the delivery on its first attempt.
Source-bearing manual commands run serially per library. If an earlier command
was interrupted, the next command finalizes its abandoned receipt with error
`manual_ingestion_interrupted`. A failed delivery has status `failed`, no
result, and a structured error.

It can still identify a work and retention disposition when failure occurred
after ingestion. An operator-skipped inbox job is reported here as a failed
delivery with error `inbox_job_skipped`. Work retention and a `work add`
completion are atomic, as are an input integration's applied result and its
corpus revision.

When the selected basis is unavailable, the delivery cannot be placed in the
window and is omitted. `missing_time_count` counts all such receipts matching
the status and channel filters. Human output states how many were omitted. To
inspect an early failure with no ingestion time, select `--by first-seen` or
`--by completed`.

Human output echoes the exact resolved range, time basis, and active filters;
reports lifecycle and retention counts; and lists the selected timestamp,
channel, status, source name, result, applied revision when present, and
retention disposition. Empty windows say `No source activity`.

JSON echoes `since`, `until`, `time_basis`, and the optional `status` and
`channel` filters. It includes delivery, lifecycle, retention, and missing-time
counts plus a `deliveries` array. Each delivery reports `source_name`,
`channel`, `status`, optional `retention` and `result`, optional work label,
optional source byte size and SHA-256, captured `source_created_at` and
`source_modified_at`, `first_seen_at`, optional `ingested_at` and
`completed_at`, optional `applied_revision`, and an optional structured
`error`. Error messages are reporting-safe lifecycle summaries; raw runner
diagnostics are never selected by this report. It exposes no storage-row
identifier or dedicated source-path field.

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

Submission resolves and validates the complete projected corpus state but does
not mutate the corpus. A result based on the same or a later revision
supersedes that work's previous pending reconciliation. An older-base result is
retained without displacing a newer pending result. `change list` includes
pending, applied, superseded, and recorded reconciliations.

With `--work`, `change show` selects that work's pending reconciliation when
one exists, otherwise its newest record. Without `--work`, it selects the sole
pending result; when none is pending, it succeeds only if exactly one work has
recorded results. `change validate` and `change apply` select pending results
only and require `--work` when more than one exists.

`change show --at REVISION` retrieves the commit at that revision. Its
`effects` are the exact material transition from the preceding revision to the
selected revision, using the same semantic entries as `diff PARENT REVISION`.
For an applied reconciliation it also shows the original graph-native request
and resolved operations. For a revert it shows the target revision and
resolved inverse. For a shake it shows the transitive-reduction request and
removed parent edges. All include the actor and timestamp.

Human reconciliation output renders public `cN` IDs alongside labels, local
creation handles, parent-edge changes, exact evidence quotations and source
context, evidence dispositions, replacements, and annotations. `change
validate` re-resolves and renders the same semantic facts without writing.
Resolved evidence reports one item per submitted selector together with its
`occurrence_count`; it never exposes the resolved ranges. Resolved operations
record what the request addressed, while `effects` report what actually
changed. An idempotent ensure may therefore appear in `resolved_operations`
without a matching effect.

`change apply` additionally requires HEAD to equal the base revision. Success
updates concepts, edges, evidence, reconciliation status,
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
and quotations must be nonempty when present. Labels and handles cannot contain
outer whitespace or control characters. `annotations` is optional and defaults
to an empty list. Annals retains annotations as descriptive context with the
reconciliation. Corpus projection, validation, and application use its
operations and evidence.

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

`quote` is required. The other fields filter its occurrences by heading and
exact immediately adjacent text. One evidence selector selects every
occurrence remaining after those filters, subject to a bounded fan-out. At
least one occurrence must remain, and each selected occurrence becomes a
separate exact-range evidence link. Use the filters when only a subset of
repeated source text is intended. Public input never contains source offsets.
Once resolved, evidence supports the concept across all of its parent
relationships. Every leaf in the final projected corpus state must have at
least one evidence link.

### Operations

- `create_concept` requires request-unique `ref`, `label`, an unordered
  `parents` array, and nonempty `evidence`. An empty parent array creates a
  derived root. Labels may duplicate existing or newly created labels.
- `add_parent` ensures one parent edge exists for `concept` without
  changing any other parent. An already-present edge is idempotent.
- `remove_parent` removes one parent edge without relocating the concept or its
  descendants. If it removes the final parent, the concept becomes a root.
- `add_evidence` ensures the evidence links selected by one or more quotations
  from the scoped work are attached to the selected concept. An
  already-satisfied mapping is idempotent.
- `remove_evidence` removes quotations from the scoped work that are attached
  to the selected concept.
- `reword_concept` preserves the public ID and requires
  `evidence_disposition: "retain" | "remove"`.
- `retire_concept` removes one concept and its incident edges. Retirement is
  nonrecursive: children survive, and a child with no remaining parents
  becomes a root. Optional `replacement` records a semantic successor but does
  not transfer edges or evidence.

The concept graph in the projected corpus state must be acyclic, have valid
endpoints and no self or duplicate edges, and have evidence on every leaf.
There is no parent priority, sibling placement, integer position, path, or move
operation.

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
off the expansion, the response reports that boundary as a frontier.
The response names its seed by ID,
stores each selected label once in `nodes`, and represents edges as
`{parent_id, child_id}` references into those nodes.

`search` matches labels and ancestor-label context. `--within cN` restricts the
search to the graph below one concept. Search results remain distinct by
public ID when labels repeat.

Paged responses contain `items` plus `page` with the requested limit,
returned count, total count, and optional `next_cursor`. The cursor is omitted
when the page is complete. Cursors are opaque and tied to the same library,
command, query, scope, and resolved revision. A later page may request a
different limit. Deterministic page order is a rendering contract, not a
conceptual ordering.

## Graph normalization

```text
annals shake [--yes]
```

`shake` computes the transitive reduction of HEAD. It removes an explicit
parent edge exactly when the child remains reachable from that parent through
another directed path. In interactive mode, the report gives the base
revision, edge counts before and after, and every edge that would be removed,
then asks once for confirmation.

Only `y` or `yes`, case-insensitively, applies the plan; any other answer or
end-of-file cancels without writing. `--yes` bypasses the prompt. With
`--json`, omitting `--yes` returns the plan with status
`confirmation_required` and exit status zero, without writing. That preview is
informational: a later invocation with `--yes` computes and applies a fresh
plan for its then-current HEAD.

Within one invocation, a confirmed shake is bound to the persistent library
identity and the exact reported HEAD revision and graph. It applies every
reported removal and creates one `shake` commit in one transaction. If the
library identity, HEAD, or its graph changes before application, it fails with
`shake_stale` and removes nothing. A graph with no removable edges skips the
prompt and remains at its current revision.

Shaking preserves concepts, evidence, every ancestor-descendant pair, roots,
leaves, label/ancestor-context search matches and ranking, and `--within`
membership. It does not preserve every original path, direct-neighbor counts,
`shared` flags, hop distances, or the revision and direct-relationship metadata
included in search responses. Transitive reduction is optional rather than a
validation invariant; a later reconciliation may add shortcut edges again.

## History

```text
annals log [--limit N]
annals diff FROM TO
annals revert REVISION
```

`log` lists newest commits first. Work retention, recorded reconciliations,
model runs, and failed attempts are absent because they are not corpus
transitions. Applied reconciliations, confirmed shakes, and reverts are
commits.

`diff` replays two revisions and reports concept creation,
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
| 5 | SQLite, integrity, or history failure |

Human rendering escapes control characters from retained text and labels.

Deployment gate identity follows the canonical database path (including a
symlink alias), or the canonical existing ancestor for a new database.
Hardlinked databases are rejected before admission. Maintenance status still
does not open or initialize the database.

## Selection and receipt output

`work list --limit N` and `change list --limit N` default to 20 and return
schema-two selection pages (`items`, `has_more`). Increase the positive limit
for more. Existing corpus cursors keep their frozen revision semantics.
Successful applied and recorded/no-change reconciliation mutations return
work, base/result revision, status, summary, operation count and recorded time
where applicable. Pending proposals retain complete review content.
`change show --work LABEL` and `change show --at REVISION` remain full reads.

`inbox retry status EVENT_ID` returns event identity, window, state, counts,
remaining work and last halt. Add `--details` for the complete original-to-child
mapping. Start/continue return the compact receipt. Without an event ID, status
lists 20 events by default, with `has_more` and `--limit N` for more.
Text and JSON select the same content; output changes do not alter corpus
history, retries, mutation authority or the accepted-account exchange.

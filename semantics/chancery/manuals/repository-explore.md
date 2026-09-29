# Semantic repositories and history

Use this feature to read a registered project's maintained vocabulary and its
immutable history. Semantics owns terminology, stable concepts, grounding, and
semantic revision order. Project source, tests, and current product documentation
own runtime behavior.

## Interfaces and selection

Select the exact registered project. Do not infer its ID from a folder name:

```sh
semantics project list
semantics repository show PROJECT
semantics repository show PROJECT --revision N
semantics repository show PROJECT --provenance --revision N
semantics repository search PROJECT QUERY --revision N
semantics repository log PROJECT --from N --to N
semantics repository diff PROJECT FROM TO
```

Omit a revision to select HEAD. Search matches case-insensitive canonical label
or meaning text and returns only matching concepts. Log and diff return complete
immutable revisions in the selected range. `diff` is a revision-range read; it
does not construct a textual difference.

Rust callers use `semantics::api::Client` for the same supported reads. Ordinary
reads return `RepositoryView`; `Client::repository_provenance` returns the full
replay representation. The client does not open SQLite or invoke hidden worker
or cutover commands. Consumers own their local projections and action policy.

The CLI returns JSON. `--json` selects compact JSON. Compact errors use
`{"ok":false,"error":{"code":"...","message":"..."}}` on stderr.
The default database is
`~/Library/Application Support/Semantics/semantics.db`. `--database ABSOLUTE_PATH`
or `SEMANTICS_DATABASE` selects an isolated database. Direct SQLite integration
is unsupported. Maintenance can fence reads before database access; see
`semantics.service`.

## Identity and meaning

A project ID identifies one registered repository across path moves. A stable,
sequential `cNNNNNN` concept ID survives wording changes, retirement, and
reopening. A positive project-local revision number identifies one atomic,
contiguous semantic commit. Provider release, output schema, persistent schema,
managed toolset, and revision number have separate meanings.

A concept has a canonical label, full meaning, active or retired state, optional
replacement, distinctions, and grounding history. Active labels are unique after
trimming, whitespace collapse, and Unicode lowercase conversion.

The immutable typed effects are:

- `define`: create the next stable concept.
- `revise`: change an active concept's label or meaning.
- `differentiate`: retain a distinction between active concepts.
- `retire`: close a concept, optionally naming an active replacement.
- `reopen`: make a retired concept active.
- `ground`: attach exact source identity and a supporting statement.
- `unground`: withdraw an earlier decision grounding while retaining its history.

Grounds identify an Annals library/event/document triple, a historical Annals
library/event/account triple, a preserved Decisions event/decision pair, or a
project-relative hashed seed. Grounds explain the recorded source of meaning;
they do not prove current product runtime behavior. New document grounding is
optional; when supplied, it must identify the selected source exactly.

## Output and completeness

Ordinary show and search use output schema 2. They include project identity,
selected revision, concept IDs, labels, full meanings, active state, replacement,
and complete distinctions for the selected concepts. `show --provenance` returns
grounds, withdrawals, and creation/change revision bookkeeping as well.
Project list includes ID, canonical current path, status, and HEAD.

HEAD replays every committed revision. Historical reads stop exactly at the
selected revision, preserving retired concepts and withdrawn groundings. A
snapshot comes from immutable typed effects, with no mutable concept projection.
These output selections change neither persistent schema nor replay behavior.
Search deliberately selects only matches; it does not report the whole repository.

## Consistency and recovery

Semantics validates an entire proposed revision against a candidate replay before
committing any effect. The revision and its ordered effects commit atomically.
Reconciliation receipt rules are owned by `semantics.reconciliation`.

Repository reads are local and do not run a worker, model, Annals, Decisions,
Conversations, or a network service. They do not use the Chancery catalog at
runtime. Opening retained state can perform a supported versioned migration;
maintenance admission therefore applies even to reads.

`project_not_found` or `revision_not_found` means the selected identity or
revision is unavailable. List projects or inspect HEAD before retrying. A replay
validation failure stops the read rather than returning a partial normal result.
Stop and use `semantics.project.operate`; do not edit SQLite, skip a revision,
or manufacture a replacement projection.

## Privacy, compatibility, and limits

Repository meanings, provenance, project paths, and source anchors are private
local state. Keep output within the selected project boundary. Repository output
contains no raw transcript, project file, command, tool result, prompt, or
credential. Grounding history is retained local provenance, not a live upstream
read dependency.

Contract 2 preserves project and concept identities and historical typed-effect
replay. No repository-size, result-count, wall-clock read objective, future
contract deprecation interval, or migration window is promised. This read feature
defines no usage or pricing unit and authorizes no registration, seed, commit,
assignment, retry, or worker invocation.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery records
command identity, time, and thread ID, not arguments, output, or outcomes.
Internal product calls are excluded. Recording errors preserve command results.

## Related contracts

Read `semantics.projects` for project lifecycle and bootstrap seeding,
`semantics.reconciliation` for changes from accepted documents, and
`semantics.service` for readiness and maintenance.

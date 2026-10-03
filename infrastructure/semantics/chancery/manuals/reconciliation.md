# Accepted-document reconciliation

Semantics saves accepted documents from one Annals decisions library as intake
for registered projects. A restricted Nucleus agent interprets relevance and
proposes effects. Semantics owns routing, cursor persistence, validation, intake
transitions, and the domain commit. Model completion does not establish a
semantic result.

## Interfaces and selected state

```text
semantics intake status [--status STATUS]
semantics intake assign EVENT_ID PROJECT
semantics intake retry EVENT_ID
```

`semantics::api::Client` supplies the corresponding supported operations and
provider-owned types. `semantics --json intake run` is the private one-shot
worker interface selected by the installed `semantics/worker` definition. It is
hidden from normal help and is not a public typed-client operation.

Intake status returns separate `annals_decision_accounts` and `legacy_decisions`
collections. The first name remains for compatibility. Statuses are `unassigned`,
`pending`, `awaiting_review`, `paused`, `processing`, `applied`, `ignored`, and
`failed`. New documents never use `awaiting_review`; legacy rows retain their
original states and decoding. Records expose fixed routing outcomes and project
assignment, not transient resolved cwd or raw dependency diagnostics.

The `semantics` CLI prints plain text by default. Pass `--json` for the existing
command-specific JSON schema. The typed client and pinned worker explicitly
request JSON. Output selection changes no records, effects, or exit statuses.

## Feed selection and completeness

Each active or paused project has its own activation and scan cursor in one
exact Annals library. Registration excludes documents at or before activation.
The scanner reads a fixed prefix after that project's cursor. It saves complete
text, original library/event/document identities, filename, digest, acceptance
time, cursor, processing state, and attempts before advancing the scan cursor
in the same transaction.

New intake IDs are local per-project identities. Embedded Annals identities stay
unchanged. A document can require one reconciliation call for each project.
No source metadata, origin anchor, conversation lookup, cwd, document layout,
or mechanical relevance rule is required for new documents. The configured
agent decides the connection to its project. Grounding is optional; a supplied
Annals document ground must name the exact library/event/document.

Annals exchange 2 returns complete text and pages of at most 200 events and
4 MiB of document bytes. A short nonempty page is not an end marker. Continue to
an empty page at the fixed watermark. Changed immutable identities, cycles,
nonadvancement, or changed replay fail closed. Retained intake and cursor state
are the coverage evidence; a 60-second timer is not a completion deadline.
No wall-clock bound from source acceptance to a semantic revision is promised.

## Instructions and execution boundary

New request preparation requires initialized private Bazaar state and a complete
`cell.prompts.semantics` selection. The default database is
`~/.local/share/bazaar/bazaar.sqlite3`; `CELL_BAZAAR_DATABASE` may select an
absolute alternate. Read `bazaar.prompts.prepare` for the shared selection format,
exact component loading, rendering, and trusted description expansion.
Semantics chooses its component set. `semantics.document.instructions` governs
relevance and interpretation. Missing or invalid selections stop preparation
before model admission. Runtime reads create no Bazaar state and use no embedded
fallback. Deployment supplies no missing prompt text.

Semantics freezes resolved instructions in the retained request. Retries retain
that selection; later text edits do not rewrite saved work. Selection version 1
and all referenced component versions preserve the migration baseline. Bazaar
owns shared text preparation. Authored meaning, runtime inputs, request and
toolset assembly, models, permissions, schemas, tool execution, commits, and
recovery remain Semantics-owned.

One job receives the complete accepted document and selected repository snapshot.
New jobs use `semantics/semantic-document-reconciliation/1` with document-specific
input and result schemas and one immutable managed tool. Nucleus uses a neutral
empty temporary cwd, workspace `none`, no shell, and no web. The temporary path
must be proved private and safe; inherited `TMPDIR`, unsafe reuse, symlinks, or
an `AGENTS.md`/`.git` control-tree ancestor cannot grant project access.

## Commit and no-change results

Semantics validates project state, base revision, effect structure, concept
identity, grounding, and replay consistency. Paused projects reject late commits.
It validates a complete candidate revision before any effect commits.

Accepted effects, their semantic revision, and the exact tool receipt commit
in one transaction before mailbox acknowledgement. Identical redelivery uses
the retained receipt. Conflicting redelivery fails. A receipt stores the call's
argument digest and exact result; it is not permission for another mutation.

An empty effect list records the exact receipt and examined HEAD as
`no_change_revision`. It completes intake as `ignored`, leaves `applied_revision`
absent, and creates no repository revision. This is an explicit interpreted
no-change result, distinct from retirement closure of unstarted work.

The worker resumes existing processing first, scans bounded pages, and processes
at most one reconciliation per invocation. A returned worker report and durable
Semantics records establish intake and commit outcomes. Clockwork process exit
and Nucleus terminal state remain separate runtime observations.

## Failure and recovery

One persisted requester/job identity survives ambiguous submission and result
transport. Recovery uses its exact retained request and mailbox correlation.
Never clear a correlation, manufacture a cursor, skip a revision, or use a direct
Codex fallback. An explicit retry creates a new attempt only after the prior
admitted job is positively terminal. Retry refuses active jobs and jobs whose
admitted state cannot be proved.

`intake assign` is an audited correction for historical unassigned intake. It
revalidates the target project marker. New documents already have per-project
intake and require no origin routing correction. Permanent retirement and its
blocked states are owned by `semantics.projects`.

Historical account and Decisions intake retain source bytes, projections,
assignment history, states, ground kinds, immutable schemas, requests,
correlations, and job decoders. Those feeds are not new-decision intake routes.
Schema 3 permits document intake per project and origin fields to be absent;
migration reinterprets no historical source or semantic revision.

A new dependency or reconciliation error ends the scheduled invocation even
if its job remains uncertain. Clockwork delays a new scheduling halt until the
shared service-health threshold; later activations retain the same correlation
and domain retry rules. A terminal failed or cancelled Nucleus job after
a semantic commit preserves that commit. `semantics.service` owns the product's
scheduled failure rules. A scheduling continuation does not retry domain work.

## Privacy and limits

Semantics retains complete accepted documents and repository snapshots for
durable recovery. They may contain private normalized conversation text.
Nucleus receives these selected bytes; it receives no project workspace, shell,
or web access. Logs contain counters, opaque IDs, and bounded product-owned
failures, with no raw dependency diagnostics, source bodies, paths, prompts,
credentials, diffs, commands, or tool payloads.

Each worker invocation is serial and processes at most one reconciliation.
There is no throughput, source-to-commit latency, storage-capacity, retention,
or general future compatibility-window promise. Chancery discovery is not a
runtime dependency. CLI usage recording requires `CODEX_THREAD_ID`; it records
identity, time, and thread ID only, and recording errors preserve results.

## Related contracts

Read `semantics.repository.explore` for concept/effect meaning,
`semantics.projects` for participation and lifecycle, and
`semantics.project.operate` for diagnosis, retry, and prompt-update procedures.

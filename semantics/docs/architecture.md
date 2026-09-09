# Architecture

## Authority

Semantics owns project registration, intake transitions, concept identity,
semantic effect validation, immutable revisions, and the final SQLite
transaction. Annals owns accepted document bytes, library identity, acceptance
order, and opaque cursors. Nucleus owns model execution and mailbox transport.
Project source, tests, and product documentation define runtime behavior;
Semantics maintains terminology and its history.

## Flow

1. Registration verifies the exact project marker and captures the current
   Annals decisions-library watermark. Earlier documents are outside that
   project's automatic intake. Moves preserve identity and cursor history.
2. A serial worker resumes existing in-flight work first, then reads a fixed
   prefix after each active or paused project's own cursor. Each new document
   becomes separate project intake. No conversation anchor, source lookup, cwd,
   or document schema is required. The agent decides relevance to that project.
3. Semantics saves the complete supplied document, original Annals identities,
   and cursor with the local project intake in one transaction. Its intake ID
   is local; the embedded event ID remains Annals' original ID.
4. One Nucleus job receives the document and the complete selected repository
   snapshot. `semantic-document-reconciliation/1` exposes one managed commit
   tool. The instructions in `document-reconciliation.md` govern interpretation.
   An empty effect list means no repository change is needed.
5. Semantics checks project state, base revision, effect structure, concept
   identities, and replay consistency. Any Annals document grounding must name
   the supplied library/event/document. Grounding is not an intake prerequisite.
   Accepted effects and the tool receipt commit together. An empty result saves
   a receipt and completes intake as `ignored` without adding a revision.

The worker performs at most one reconciliation per invocation. A document can
therefore require separate agent calls for several projects. No mechanical
origin-based filter silently discards it before those agents can interpret it.
Paused projects retain intake but cannot commit until resumed.

The job uses a neutral temporary directory, workspace `none`, no shell, and no
web. It receives the supplied document rather than resolving a source thread.
Ambiguous transport recovery reuses the same requester and job. Retry creates a
new attempt only after positive terminal evidence. Historical account and
Decisions jobs retain their original payloads, routing, tools, and replay rules;
they are not alternative sources of newly produced decisions.

## Serial service

Clockwork key `semantics/worker` requests one hidden `intake run` every 60
seconds, without run-at-load or an activation timeout. Its immutable definition
records the exact release ID and root and pins `/bin/sh` plus the release-local
runner by SHA-256. Semantics' release manifest and retention rules own the
sibling payload and full release integrity. The definition uses a scrubbed
environment and skips overlap. A cross-process Semantics lock serializes work;
an independently started overlapping invocation does nothing. Each run resumes
one processing item first, scans bounded document pages, and applies
at most one reconciliation. Pausing a project prevents late proposals from committing.

A maintenance marker outside the release prevents runners from starting domain
work during deployment, uninstall, or recovery after failure. The marker must
belong to the current user, have mode `0600`, and have no hard links.
Lifecycle tools refuse any other shape and never truncate an existing marker. A successful
deployment may retain an authenticated marker/receipt pair for an outer
cutover; a later successful same-release invocation releases only that pair.
An unrelated unreceipted marker is preserved. Uninstall and an unprovable
rollback retain the gate. Existing product log files must likewise
be current-user-owned regular non-hard-linked files; deployment makes their
mode `0600` without truncating content before definition registration.

Clockwork records schedule, definition, binding, process, and scheduling-incident outcomes. It
does not inspect Semantics domain state or ingest product log bodies. A zero
process exit reports only that the one-shot invocation returned successfully;
the Semantics database and worker report remain authoritative for intake and
commit outcomes.

Service stdout contains only counters and opaque identifiers; stderr contains
only bounded product-owned failure codes and messages. Raw dependency errors,
document text, project content, conversation
text, anchors, paths, diffs, commands, tool output, credentials, and Nucleus
prompts must not enter service logs.

## Failure boundaries

Annals cursors advance in the same SQLite transaction that durably records an
project document intake. Relevance is resolved by the agent afterward. Each page is fixed to one watermark;
empty pages cannot advance and changed immutable identities fail closed.
Repository revisions and typed effects are append-only. New failed or
unassigned intake and all legacy states stay explicit. Separate legacy and
account mailbox receipts make repeated delivery idempotent and reject
conflicting replay. Operator retry refuses a prior Nucleus job that is active
or whose admitted state cannot be established.

## Rust interfaces

`semantics::api` is the provider-owned import surface for repository, project,
revision, grounding, intake, and diagnostic output types plus a typed CLI
client. CLI output uses the same types. The client implements supported public
commands; it does not open SQLite, run hidden worker/cutover operations, or
replace Semantics validation. A failed doctor still returns its typed check
report. Consumers own their local projections and action policy.

Upstream adapters use `annals-api` and `krisis_api::lifecycle` clients and
exported response types, then convert them to Semantics-owned document and
legacy intake records. Exchange contract 2 supplies the complete text. They do not define upstream wire replicas. Existing
local source traits retain worker policy and synthetic-test substitution.

## Scheduled failure policy

Semantics configures Clockwork definition schema 2 for `semantics/worker` with
`[failure] on_abend = "halt-until-approved"`. A worker reports only failures
encountered by its current invocation. `intake run` prints its report and returns
nonzero when `error_event_id` is present. Retained failed intake is not rescanned
as a new incident. Normal mailbox waiting, an overlapping worker, a paused
project, maintenance, or no eligible intake does not itself constitute an abend.
A returned dependency or reconciliation error is an abend even when its job
remains in progress awaiting definitive recovery evidence.

A terminal failed or cancelled Nucleus job after a semantic commit preserves
that commit and reports its exact job ID to Clockwork. The report contains a
bounded code and opaque identity, never document text or raw runtime diagnostics.
The runner forwards Clockwork's correlated activation context through its
otherwise scrubbed environment. It does not poll historical completed jobs.

Clockwork owns the durable halt, future admission, and one retained notification
through `HOME/.local/bin/email`. Inspect `clockwork incident list
semantics/worker` and `clockwork incident show INCIDENT_ID`. Only explicit
approval followed by `clockwork binding resume semantics/worker INCIDENT_ID`
releases that halt. Definition switches, deployment, project resume, and intake
retry preserve it. The last two operations remain Semantics-owned domain controls.
Scheduling continuation creates no retry and cannot authorize a new request
while a prior Nucleus job remains uncertain. Schema-one definitions acquire the
new policy only when a schema-two definition is explicitly selected.

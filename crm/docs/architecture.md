# Architecture

CRM is a private local library of employment-related cases. A case can begin
with a company, location, person, posting, introduction, or any other useful
lead because version 0.1 does not force those inputs into a universal entity
model. It retains raw UTF-8 Markdown and lets a bounded steward turn new
information into a complete next case revision.

The product consists of a short-lived CLI, one local SQLite database, and a
hidden worker launched for explicitly queued or resumed work. It installs no
daemon, LaunchAgent, or schedule. Apart from its typed Nucleus requester
connection, it has no runtime connection to another product or network
service.

## Authorities

| Authority | Owns | Does not own |
| --- | --- | --- |
| Caller | The text it supplies, source labels, choice to create or tell a case, and any real-world contact action | The steward's generated revision or Nucleus execution state |
| CRM | Case, delivery, immutable revision, and steward-update/attempt identities; stage; advisory retention and display; validation; atomic commits; retry admission; and deterministic reads | Truth of cited sources, permission to contact someone, or proof that contact occurred outside CRM |
| Cited source | The external fact or record referenced by the caller | CRM's retained interpretation or update history |
| Nucleus | Agent admission, authentication, supervised execution, job/output records, and durable managed-tool transport | CRM case state, domain success, update retry policy, or the meaning of a stage |
| Steward agent | One bounded proposed full replacement revision | Authority to bypass CRM validation, contact anyone, or make final prose a domain result |
| Chancery | Installed contract discovery and exact promise resolution | CRM runtime execution or case data |

## Case and intake flow

`case new` creates a stable case and immutable revision one. Its optional input
is raw UTF-8 Markdown transported from a regular file or standard input. If no
input is supplied, CRM generates a title plus `Current picture`, `People`,
`Chronicle`, and `Open threads` headings. This is a suggested editorial shape,
not a document schema: CRM accepts, stores, and revises Markdown without
requiring or parsing any heading. The case's initial stage defaults to
`research` unless the caller supplies one of:

```text
research | warranted | contacted | connected | helped | closed
```

The stages are compact claims in the case ledger. In particular, `warranted`
means the retained current revision considers contact worthwhile; it is not an
authorization or blocking gate. `connected` and `helped` are likewise CRM
claims based on supplied information, not independent observation of another
person.

`tell CASE_ID INPUT` accepts one new free-form UTF-8 delivery. In one database
transaction CRM retains its exact text as SQLite `TEXT`, records its digest and
optional display name/source reference, and creates one queued update. It does
not retain the input path and never writes a content file. Once that commit is
durable, `tell` launches the hidden worker and returns the queued update without
waiting for agent execution; machine output includes both the update and
delivery identities. Failure to launch cannot erase the queued update. The
explicit recovery surface is `update resume`.

Hidden drainers serialize through a database-resident lease. A drainer launched
behind a live owner waits for at most two seconds. This bound is safe because
a drain owner atomically either claims the next eligible queued row or releases
its lease, while a resume owner atomically releases and requests one replacement
drainer if eligible work is waiting. Already-committed intake therefore stays
with a drain owner or receives a post-resume worker; intake after release has a
free lease for its own child. A drain attempts each pre-existing unsettled item
once and continues into eligible queued work before surfacing an unresolved
per-item diagnostic.

`--source` is an opaque caller-supplied reference. CRM neither opens nor
refreshes it. This lets ordinary web or CLI research, a job posting, a meeting
note, or a human introduction use the same narrow intake line without giving
CRM a browser, crawler, address book, or guessed-contact pipeline.

## Hidden steward

The worker claims one update and freezes the case's current base revision and
the new delivery before submission. That update is one steward attempt. It uses
requester program `crm`, requester identity `case-steward:UPDATE_ID`, one unique
Nucleus job identity per update, and the immutable toolset
`crm/case-steward/1`.

The steward is told about the same four suggested sections, but may preserve or
choose a better case-specific organization. CRM validates only the bounded
complete Markdown value, not its headings or prose structure.

The closed Codex invocation uses model `gpt-5.6-terra`, medium reasoning, and a
1,200-second timeout. It places the frozen base and delivery in the prompt,
uses a neutral absolute working directory, workspace access `none`, local
execution and web search disabled, no launch context, and exactly one managed
tool: `submit_case_revision`. The tool supplies the frozen `base_revision`
guard plus exactly four revision fields:

- complete replacement Markdown;
- one stage from the fixed enum;
- a nullable advisory; and
- a summary.

The prompt and tool result are private retained execution data. The neutral
working directory is a transient empty directory next to the selected database
and is removed after terminal execution is observed; case content remains in
SQLite. There is no read tool, shell, web tool, source adapter, or second
execution path.

CRM validates the tool call and commits only if the selected case still has
the frozen base revision. The new immutable revision, exact tool receipt, and
update's committed-revision reference become durable atomically. After posting
that byte-identical result, the worker continues through Nucleus terminal
observation and retains both the post acknowledgment and runtime outcome. That
guarded database commit—not Nucleus completion or model prose—is domain
success. A later transport, daemon, or harness failure becomes a visible
diagnostic and does not undo an already committed revision. A stale base or
invalid call commits no revision and remains inspectable as failed work.

## Recovery

`update wait` observes one update and, once on entry when domain or runtime
settlement still needs work, actively launches the queue drain or same-update
recovery worker. It then only polls, times out after 1,200 seconds by default,
and never grants retry authority. An `applied` update is already domain
success; wait nevertheless remains active until CRM has retained a terminal
Nucleus observation, so post-commit diagnostics cannot disappear.
`update resume` synchronously processes queued or interrupted work without
inventing a new update when the exact attempt can still be resumed. It may
recover a durable pending tool call idempotently. `update retry` is allowed
only after a `failed` or `lost` update and creates a successor update with a
new requester/job identity, the same immutable delivery identity/text, and a
`retry_of` link. CRM never retries automatically.

Ambiguous admission reuses only a byte-identical request under the same job
identity. An accepted tool call is durably correlated to that job and call;
byte-identical redelivery returns its stored result, while conflicting reuse
fails closed. Before dispatching any pending call, CRM rechecks that its exact
job is still nonterminal. A Nucleus restart may make an active harness attempt
`lost` and does not authorize a new one. If the revision committed first, it
remains successful and the runtime loss is retained separately.

## Advisory and proof boundary

A non-null advisory is displayed conspicuously by every surface that consumes
the relevant revision: case list, search, history and show, plus tell
acknowledgment and update list/show/wait/resume/retry. Applied update views use
their committed revision; other update views use their frozen base when
assigned and otherwise the current case revision.
Human output uses `ATTENTION — STEWARD ADVISORY (NON-BLOCKING)`; JSON includes
both `attention: true` and the advisory text. It is durable evidence about the
steward's caution, but it never blocks case intake, inspection, stage changes,
worker recovery, or any caller-owned real-world action.

CRM can substantiate that particular input bytes were retained, a particular
bounded AI run proposed a revision, and CRM accepted it under a specific base
and tool receipt. It cannot by itself substantiate that a source was true, a
message was sent, another person replied, or employment help occurred. Those
facts must arrive through a caller-supplied delivery with an appropriate source
reference and remain attributable to that source.

The supported version-0.1 reads expose immutable case revisions,
`source_update_id`, update/delivery identity, and Nucleus requester/job
correlation. Raw delivery bodies, persisted request JSON, and mailbox receipt
JSON are retained for exact execution and recovery but have no public
show/export command. Direct SQLite access is not a supported consumer surface.

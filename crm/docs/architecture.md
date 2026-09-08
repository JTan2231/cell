# CRM architecture

CRM uses a short-lived CLI, one private SQLite library, and a hidden worker.
The caller supplies case information. A bounded Nucleus steward proposes a
complete replacement revision. CRM validates and commits it.

## Ownership

| Participant | Owns |
| --- | --- |
| Caller | Supplied text, source labels, corrections, and real-world contact actions |
| CRM | Cases, profiles, deliveries, immutable revisions, updates, tool receipts, and validation |
| Nucleus | Agent admission, authentication, execution, and managed-tool transport |
| Steward | One proposed replacement revision under the frozen case basis |
| Chancery | Installed contract discovery |

CRM has no daemon or schedule. Nucleus is its only runtime service connection.
An advisory informs the caller; it never authorizes or blocks an operation.

## Profiles

Profile entries hold reusable career material as exact Markdown. An explicit
write creates or replaces the current entry without AI. It does not alter a case
or supply automatic steward context. The caller must explicitly include profile
text in a case update when needed.

Input files and stdin are transport. CRM stores content as SQLite `TEXT` and does
not retain input paths or maintain a parallel content directory.

## Case update flow

```text
case new --> case + revision 1
                 |
tell --> delivery + queued update --> hidden worker --> Nucleus
                 |                                      |
                 +<-- guarded revision commit <--- proposed revision
```

`case new` stores the initial revision. With no supplied body, it creates a
suggested outline. Headings are editorial hints; CRM does not parse or require
them. Stages classify the recorded case history and do not establish contact
permission or external outcomes.

`tell` commits exact input and one queued update in the same transaction.
It then starts the worker and returns. A launch failure leaves the update
available for `update resume`. Source references are caller-supplied labels;
CRM does not fetch or refresh them.

One database lease serializes drainers. At the end of a drain, an immediate
transaction either claims eligible work or releases the lease. Resume releases
the lease and detects waiting work atomically, then starts a replacement drainer
when needed. A contender waits at most two seconds. Each drain attempts existing
unsettled work once and processes eligible queued work before returning an
unresolved diagnostic.

## Steward and commit

The worker freezes the case revision and new delivery before submission. It
persists the exact request with requester program `crm`, requester identity
`case-steward:UPDATE_ID`, and one Nucleus job ID. The immutable toolset is
`crm/case-steward/1`.

The invocation uses `gpt-5.6-terra`, medium reasoning, and a 1,200-second timeout.
The prompt contains the frozen revision and delivery. Workspace access, local
execution, web search, and launch context are disabled. The only managed tool is
`submit_case_revision`.

The tool supplies the base revision and four content fields: complete Markdown,
stage, nullable advisory, and summary. CRM checks the base and content before
one transaction commits the revision, case head, tool receipt, and update result.
That transaction establishes domain success.

The worker posts the exact durable result and continues until it records Nucleus
terminal state. A later runtime failure becomes a diagnostic and does not undo
the revision. An invalid call or stale base changes no case content.

The neutral working directory is a temporary empty directory beside the database.
It is removed after terminal observation. Prompts and tool results remain private
execution data.

## Recovery

`update wait` may launch drain or same-update recovery once on entry, then polls.
Its default timeout is 1,200 seconds. An applied update remains under observation
until terminal Nucleus state is retained. Wait does not create retry authority.

`update resume` continues queued or recoverable interrupted work. A failed or
lost update needs explicit `update retry` to create a successor with a new job
ID, the same delivery, and a `retry_of` link. CRM never retries automatically.

An ambiguous submission reuses only the exact request and job ID. A repeated
managed call returns its saved result; conflicting reuse fails. CRM checks that
the job remains nonterminal before servicing a pending call.

## Advisories and public reads

Every view that consumes a revision displays its complete advisory. Applied
update views use the committed revision; other updates use their frozen base
or the current case when no base is assigned. The human banner and JSON attention
fields identify it as non-blocking.

Public reads expose profiles, case history, source-update links, update and
delivery IDs, and Nucleus correlations. Retained raw delivery bodies, request
JSON, and mailbox receipt JSON have no public export command. Direct SQLite
access is not a supported consumer interface.

See [commands and output](cli.md), [stored records](data-model.md), and
[the Rust interface](rust-api.md) for exact formats and limits.

Deployment holds are separate from operator pauses. See
[installation and maintenance](system-installation.md#coordinated-deployment-maintenance)
for admission, migration, and recovery.

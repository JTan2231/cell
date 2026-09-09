# Nucleus ecosystem operator manual

Use this manual for shared system boundaries, coordinated maintenance, and
requester integration. Product references define each product's records and
operations. The installed `nucleus manual` command prints this versioned document
without contacting the daemon.

- [Choose a system](#choose-the-system-by-the-intended-outcome).
- [Check compatibility](#compatibility-model).
- [Coordinate deployment](#shared-ci-release-and-deployment-coordination).
- [Quiesce affected work](#quiesce-before-work-that-cannot-tolerate-a-lost-attempt).
- [Add a requester](#add-a-new-requester).
- [Diagnose a failure](#diagnosis-and-recovery).
- Use [Nucleus installation and recovery](/Users/joey/rust/cell/nucleus/docs/system-installation.md)
  for service setup, authentication, backup, and restore.

## Choose the system by the intended outcome

Read the installed catalog. Compare the intended outcome with all entry titles
and summaries. Read every plausible contract before choosing an interface:

```sh
chancery list
chancery show ENTRY_ID
```

For a complete outward promise or a design reliance, resolve the selected ID:

```sh
chancery resolve ENTRY_ID
```

Keep unsupported, unspecified, not-applicable, and undeclared claims distinct.
A schema or implementation detail cannot supply a missing promise. Chancery
reads documentation; it does not establish readiness, authorize an action, or
execute the interface. If no entry fits, perform ordinary work normally.

| System | Owned outcome | Product reference |
| --- | --- | --- |
| Todo | Retained concerns, routing decisions, situation assessments, and designs | [Todo](/Users/joey/rust/cell/todo/README.md) |
| CRM | Employment cases, immutable case revisions, and editable career entries | [CRM](/Users/joey/rust/cell/crm/README.md) |
| Cast | Discovered companies, public jobs, collection outcomes, and exports | [Cast](/Users/joey/rust/cell/cast/README.md) |
| Platter | Prepared job packets, frozen editions, and email outcomes | [Platter](/Users/joey/rust/cell/platter/README.md) |
| Annals | Retained sources, library instructions, interpretations, and corpus history | [Annals](/Users/joey/rust/cell/annals/README.md) |
| Conatus | Exact want intake and associations with accepted decisions | [Conatus](/Users/joey/rust/cell/conatus/README.md) |
| Email | Fixed-recipient submission and received-account mail reads | [Email](/Users/joey/rust/cell/email/README.md) |
| Mentor | Daily problems and independent answer critiques | [Mentor](/Users/joey/rust/cell/mentor/README.md) |
| Paperboy | Daily reports from local conversation history | [Paperboy](/Users/joey/rust/cell/paperboy/README.md) |
| Conversations | Local Codex task metadata and normalized message reads | [Conversations](/Users/joey/rust/cell/conversations/README.md) |
| Krisis | Decision identification, coverage, and delivery to Annals | [Krisis](/Users/joey/rust/cell/decisions/README.md) |
| Semantics | Registered project terminology and its revision history | [Semantics](/Users/joey/rust/cell/semantics/README.md) |
| Usher | Declared Cell membership | [Usher](/Users/joey/rust/cell/usher/README.md) |
| Clockwork | Scheduled process activation and runtime history | [Clockwork](/Users/joey/rust/cell/clockwork/README.md) |
| Nucleus | Constrained agent execution, authentication, and job history | [Nucleus](/Users/joey/rust/cell/nucleus/README.md) |
| Annals Usage | Live Annals-attributed consumption and account allowance | [Usage reporting](/Users/joey/rust/cell/annals/docs/telemetry.md) |

## Topology and authority

```text
requesting products --> Nucleus --> isolated Codex app-server
       |                   |
       |                   +-- authentication, jobs, output, tool mailbox
       +-- domain records, validation, retries, and recovery

Clockwork --> registered product programs
Conversations --> normal-user Codex App Server
Krisis --> dedicated Annals decisions library --> Semantics and Conatus
Cast --> Platter <-- CRM career entries
Platter, Mentor, Paperboy --> Email --> Resend
installed product releases --> Chancery documentation
```

Krisis uses `krisis/decision-document/1` for its sole active observer production
path. It freezes full normalized conversation through a completed exchange,
accepts a yes/no verdict and summary, and renders the source into Markdown.
The saved run protects request and result recovery; the observer commits
coverage and a target-bound document outbox. Local `document build` and
`document render` use the same builder without delivery or observer coverage.
Krisis marks an observation failed on its first processing error and continues
with other work. Explicit observation retry preserves uncertain Nucleus request
identity and pending Annals document identity. `krisis health` reports worker
activity and its age; retained observation failures do not make the worker
unhealthy. See [Krisis source documents](/Users/joey/rust/cell/decisions/docs/source-documents.md).

Annals exchange contract 2 accepts the handed text without a decision schema
or source lookup. It owns ordinary storage metadata and returns complete accepted
documents through its dedicated feed. Conatus forwards those exact bytes to its
library. Semantics supplies each post-activation document to each participating
project's reconciliation agent. Its `semantic-document-reconciliation/1` toolset
allows an empty result without creating a semantic revision. Interpretation and
connections belong to the agents and their instructions. Historical account
jobs retain their original immutable tool contracts. Persistent schemas are
Krisis 6, Annals 7, and Semantics 3; these migrations preserve accepted history
and cursors. Krisis refuses the document cutover while old handoffs or
classification jobs are in flight.

Conatus pins its Annals executable and its Clockwork definition pins Conatus.
After replacing these releases, repeat `conatus init` with the existing library
selections to verify their identities and replace only the Annals executable
pin. Then register and select a definition from the new Conatus release.
Preserve the existing pause and schedule activation states during this cutover.

Each product owns its data and success rules. Nucleus owns execution. A completed
model turn does not establish a product result. A later runtime failure does
not erase an already committed domain result.

Clockwork owns activation, direct-process history, and the configured response
to an abend. Products own definition configuration, work, locks, idempotency,
logs, and domain recovery. A process exit does not establish product success.
Products report a failed scheduled operation even when its domain result was
already committed; the commit remains valid.

Schema-two definitions default to `halt-until-approved`. Clockwork durably
blocks that binding and queues one metadata-only failure alert through Email.
Products may explicitly configure `continue-next-activation`. Empty queues,
deployment holds and declared readiness waits are expected product outcomes;
they do not become failures solely because no work was performed.

Inspect an incident and its product evidence before approving its exact ID with
`clockwork binding resume KEY INCIDENT_ID`. Approval clears that halt only. It
does not enable a disabled binding, resend uncertain email, retry a model job,
or discard retained work. Disable, reinstall, and definition selection preserve
halts. Product recovery still controls whether a particular attempt is safe.

| Definition | Product-owned rules retained |
| --- | --- |
| `annals/inbox`, `annals/decisions-inbox` | Operator pause and readiness waits; archive a failed source once, then stop the scheduled batch. |
| `conatus/update` | Preserve source/feed identity; stop after the first failed update stage. |
| `krisis/observer` | Preserve coverage and pending document identity; explicit observation retry. |
| `semantics/worker` | Preserve committed revisions and report a new failed reconciliation. |
| `mentor/worker` | Preserve frozen message/key limits; cleanup-only expiry remains available while scheduling is halted. |
| `paperboy/daily` | Explicit failed-brief retry and uncertain-send reconciliation. |
| `platter/daily` | Declared source-readiness deferrals; preserve edition bytes and uncertain-send recovery. |
| `todo/daily-email` | Preserve digest occurrence keys and credential loading; skip deliberate deployment holds. |

Before migrating Clockwork runtime state, capture and disable existing bindings,
settle activations, and use its explicit backup-bearing migration. Old schema-one
definitions retain their historical policy until products generate and select
schema-two definitions. Preserve each captured enabled state and operator pause
when rebinding. Todo's installer retires its fully attributed legacy LaunchAgent
before selecting its Clockwork successor. Follow the Clockwork installation
contract for compatible binary/state recovery and notification retry limits.

Nucleus owns its private Codex credentials. Requesters do not copy or refresh
them. Conversations reads normal-user history through App Server; it does not
read the isolated Nucleus job store. Annals Usage reads supported Nucleus output
and Annals attribution records without owning either store.

Email owns its transport and credential loading. Its receipt means provider
acceptance, not final inbox delivery. Todo's daily email uses its own direct
Resend path and does not depend on Nucleus execution.

Source colocation and a shared Cargo workspace do not merge product databases,
credentials, release units, or runtime authority.

## Read supported and installed state separately

Use versioned contracts for supported behavior. Use live interfaces for the
selected installation:

```sh
nucleus --version
nucleus health
nucleus service status
nucleus account --wait 0
chancery doctor
```

`nucleus health` prints readiness and exits nonzero unless the daemon is
compatible, authenticated, and accepting jobs. Its execution fields report
`maxActiveJobs=8`, occupied `activeJobs`, and `availableSlots`.
`acceptingJobs` describes admission, even when all slots are occupied.

`authentication_busy` identifies credential-operation contention. An active
job alone does not make an account read busy or prove a bad credential.
A held deployment uses the exact owner's `maintenance health RUN_ID` interface;
ordinary health remains strict.

Do not maintain a dated installed-version table here. A source checkout or
catalog entry does not establish the currently running release.

## Compatibility model

| Boundary | Required treatment |
| --- | --- |
| CLI and daemon release | Install matching candidates. Inspect both with health and version reads. |
| Public invocation protocol | Deploy additive daemon support before requesters emit it. Use a new protocol for incompatible meaning. |
| Capability contract | Version incompatible documented behavior and review consumer dependency bounds. |
| Codex harness | Prove the exact adapter version before replacing the configured executable. |
| Persistent schema | Back up and migrate explicitly. Restore only a compatible database and binary pair. |
| Immutable schemas and toolsets | Give changed meaning a new identity. Keep decoders for retained jobs. |
| Requester client | Rebuild when consumed types or behavior change. Shared source does not require lockstep deployment. |

If old and new protocol forms cannot coexist, stop admission and settle all
affected work before the coordinated cutover. Credential recovery stays separate
from program and database rollback. Never restore an older credential as a side
effect of either rollback.

A decoder repair may expose output from an old completed job without another
model attempt. Missing observations remain a gap. The repair does not change
requester terminal records or authorize a retry.

## Shared CI, release, and deployment coordination

From the Cell root, use the default gate for routine validation:

```sh
./ci.sh
```

It selects outstanding changed products relative to `HEAD`, including deletions
and both paths of renames. Explicit product arguments limit product coverage.
A scoped success does not establish full repository validation. Use `--all`
only when the user explicitly requests full CI.

CI rejects source changes during execution as stale. Linked worktrees share the
CI broker and compiler resources. For selection, output, and recovery details,
see [CI selection](/Users/joey/rust/cell/pipeline/README.md) and
[the CI broker](/Users/joey/rust/cell/ci_broker/README.md).

Publication and deployment are separate effects. A product release command
changes versions, commits, tags, and pushes. CI does not publish. Release and
deployment preparation build and seal production artifacts; they do not rerun
CI or turn a build receipt into test evidence.

### Selection-only Cell deployment

Preview the selected systems, then use the coordinator when deployment is authorized:

```sh
./deploy.sh plan SYSTEM...
./deploy.sh SYSTEM...
```

The coordinator selects a local `main` commit and prepares immutable candidates.
Product declarations order selected releases and identify affected installations
to hold. Dependencies do not silently upgrade unselected products. Annals includes
Usage; `decisions` aliases `krisis`.

The shared sequence is:

1. Prepare and verify all selected candidates before maintenance.
2. Hold affected requester admission and settle domain work.
3. Hold and drain Nucleus after requester continuation work has finished.
4. Apply selected candidates in dependency order.
5. Verify selected candidate identity and affected-only installed readiness.
6. Release requester holds, then release Nucleus last.

A hold belongs to one run, survives process exit, and does not expire. Releasing
it preserves other holds, operator pauses, and disabled schedules. Drain must
include durable unfinished work and associated Nucleus jobs.

Verification creates no model jobs or synthetic domain records. Recovery must
establish a coherent prior or candidate installation before releasing admission.
An uncertain apply is not repeated blindly. Matching files and health alone do
not prove replacement of a resident Nucleus daemon.

Unproved recovery retains the product hold and identifies its owner. A successful
recovery still reports the original deployment failure. Cleanup failure does not
erase installation success. The coordinator's final result distinguishes these
outcomes.

The coordinator retains no public deployment history or resume interface.
Product holds and recovery backups remain until resolved. Release cleanup
preserves current releases and exact pins held by configuration, schedules, or
processes. Unknown or incomplete pin inventories stop deletion.

See [Cell deployment](/Users/joey/rust/cell/deployment/README.md) for candidate,
locking, cleanup, and interrupted-operation details. Each product's installation
contract owns its state, migration, scheduler, and recovery procedure.

### Migration to coordinated deployment

An older executable may ignore a candidate's admission hold. Install a compatible
maintenance-capable release through the product's existing procedure first.
Capture enabled schedules and operator pauses before stopping admission.
Settle domain work and Nucleus jobs, install the compatible release, and verify
its maintenance interface. Restore only the captured enabled state after readiness.

New databases, credential provisioning, schedule policy, and domain imports use
their explicit product operations. A temporary maintenance pause does not replace
the operator's original intent.

## Quiesce before work that cannot tolerate a lost attempt

For coordinated deployment, use the owned holds above. For an attended service,
storage, or authentication operation:

1. Identify every affected requester and its admission controls.
2. Record current pauses and each schedule's selected digest and enabled state.
3. Prevent new manual and scheduled work through the product interfaces.
4. Let active domain work and Nucleus `accepted`, `running`, and
   `waiting-on-requester` jobs settle.
5. Cancel an exact job only when abandoning that attempt is intended.
6. Perform the operation and verify Nucleus and affected product readiness.
7. Restore only controls that were enabled before the operation.

For example, pause each affected Annals library through its own inbox command.
Inspect Clockwork bindings before disabling them. Never enable a disabled binding
or load a legacy scheduler beside its Clockwork successor. A retained source or
an empty process list does not prove that durable work has settled.

Todo's daily-email timer is not a Nucleus requester. Do not pause it merely to
quiesce Nucleus. Preserve Krisis coverage and outbox state, Semantics cursors,
and every requester's recovery records.

Graceful shutdown requests cancellation. Startup marks unfinished Nucleus
attempts `lost`. The requesting product decides whether another attempt is safe.

## Add a new requester

Define the durable product result before choosing an invocation. The requester
owns its data, validation, duplicate handling, retry policy, and recovery.
Nucleus receives only execution policy, correlation, and tool definitions.
There is no `nucleus project create` registration step.

The normal flow is:

1. Check required Nucleus protocol, harness, account, and admission capabilities.
2. Register immutable schemas and toolsets.
3. Persist the exact request and its domain-run correlation.
4. Submit it and tolerate accepted work waiting for an execution slot.
5. Validate and apply managed-tool calls through the product backend.
6. Retain each exact result before posting it to the mailbox.
7. Observe terminal runtime state and determine success from product records.

An ambiguous submission reuses only the same job ID and byte-equivalent request.
A new attempt gets a new job ID. Timeout begins after slot acquisition;
`waiting_on_requester` keeps its slot. A requester must isolate or serialize
concurrent writes to the same target. Nucleus does not detect those conflicts.

Keep source content separate from trusted instructions. Declare workspace,
local execution, web access, model, reasoning, timeout, and tool permissions
explicitly. Do not add an undocumented direct-Codex fallback.

See [the requester contract](/Users/joey/rust/cell/nucleus/chancery/manuals/requester-integrate.md)
for the complete integration procedure and required checks, and
[the runtime contract](/Users/joey/rust/cell/nucleus/docs/runtime-contract.md)
for request, tool, and output formats.

### Provider-owned Rust interfaces

Use each provider's public types, codecs, and supported client. Convert imported
values into local domain models as needed. Do not copy provider wire structs,
private SQL, or command parsers.

Nucleus and Conversations expose their existing libraries. Other products expose
focused `api` modules. `krisis-api` owns historical account and retained Decisions lifecycle
exchange. `annals-api` owns acceptance, feed, and Usage interfaces. General
operations use `decisions::api` and `annals::api`.

A partial provider view does not perform full validation. Clients retain the
effects, failures, and transport rules of their operations. Publish incompatible
exports with the provider and update affected consumers.

## Guarded change playbooks

Identify the owning product before changing a boundary. Update its behavior,
contracts, and affected integrations together. Update this manual only when
shared facts or procedures change.

### Routine Nucleus patch

1. Identify affected public meaning, schema, harness, and requester obligations.
2. Update the owning code and documentation, then run the required product gate.
3. Publish only when authorized. Release requires clean `main` synchronized
   with `origin/main` and creates the commit and tag.
4. Quiesce affected work and deploy matching CLI and daemon candidates.
5. Verify strict health and requester readiness before restoring admission.

Use [Nucleus installation](/Users/joey/rust/cell/nucleus/docs/system-installation.md)
for the exact installer and rollback procedure.

### Exact Codex upgrade

Inspect the candidate executable, version, model catalog, app-server schema,
and every consumed method and isolation rule. Update the adapter and its
compatibility checks before deployment. After cutover, health must identify the
exact executable, supported version, and required capabilities.

### Public protocol or client change

For additive support, deploy the accepting daemon before new callers. For an
incompatible change, retain both forms during migration or quiesce all affected
requesters. Update core types, client, daemon routes, runtime documentation,
examples, and contract checks together.

### Nucleus database schema change

Provide a versioned migration from every supported prior version. Define the
transaction boundary, post-commit maintenance, backup, and rollback procedure.
Before cutover, settle requesters and pending calls, stop Nucleus, and take the
required consistent backup. Validate retained jobs, output, and mailbox integrity.

Old binaries must not open an incompatible new database. Recovery across that
boundary requires a matching database and binary pair. See
[schema recovery](/Users/joey/rust/cell/nucleus/docs/system-installation.md#schema-recovery).

### Requester schema, toolset, prompt, or permission change

Publish new immutable identities when registered meaning changes. Keep historical
decoders. The requester owns prompt, model, reasoning, timeout, workspace, tools,
and launch-context policy. Use new job IDs for new attempts and check the
required Nucleus capabilities and domain behavior.

### Authentication or service-ownership change

Prevent new credential consumers and let active sessions finish. Preserve one
private credential authority, the login/session barrier, serialized refresh,
and atomic promotion of validated staged credentials. Do not distribute managed
refresh tokens to workers. Verify account and service readiness before resuming.
Binary and database rollback must not restore older authentication bytes.

## Diagnosis and recovery

Start with `nucleus health`, `nucleus service status`, and the requesting product's
status. Select the exact requester and job before reading detailed output:

```sh
nucleus jobs list --requester PROGRAM --requester-id REQUESTER_ID
nucleus jobs status JOB_ID
nucleus jobs wait JOB_ID --timeout 60
nucleus jobs show JOB_ID
nucleus jobs logs JOB_ID
```

Status summarizes one current observation. Wait returns `terminal` or `timeout`;
a timeout does not cancel, retry, or make the job terminal. Job and mailbox reads
are successive observations, not one atomic snapshot. Detailed output can contain
private prompts, sources, and tool values.

| Observation | Action |
| --- | --- |
| Service unavailable | Check socket selection, LaunchAgent, and Nucleus logs. |
| Unsupported harness | Restore the supported executable or complete the adapter upgrade. |
| `model_auth_unavailable` | Follow attended authentication recovery after quiescence. |
| `authentication_busy` | Wait for the credential operation; do not replace credentials. |
| `waiting_on_requester` | Inspect pending calls and the product's recoverable work. Do not invent a tool result. |
| `lost` attempt | Inspect domain state before the requester creates another attempt. |
| Runtime failure after a domain commit | Preserve the committed result and report the runtime diagnostic. |
| Runtime completion without the required record | Follow the product's failure policy. |
| Unresolved deployment hold | Use that product's retained recovery procedure. |

Platter cancels its exact model job and stops daily preparation on a renderer
execution failure. After repair and terminal job status, an authorized
`platter prepare CAST_JOB_ID --fresh` creates new source capture and model jobs.
It retains old runs without passing their outputs or error context to the new
preparation. See [Platter preparation](/Users/joey/rust/cell/platter/chancery/manuals/packet-prepare.md)
for eligibility and accepted-resume limits.

After a shared change, verify matching programs, service status, exact harness,
account access, and affected product readiness. Release only holds and pauses
owned by the operation. Preserve pre-existing disabled schedules.

## Where facts and changes belong

Keep each full explanation with its owning product. This manual owns shared
topology, authority, compatibility, coordination, and recovery order. Product
references own record meaning and exact operations. Chancery manuals remain
self-contained for installed use.

Use short active sentences and descriptive headings. State which records or
operation a count, timestamp, or failure describes. Remove duplicate explanations,
completed-work narratives, test-result reports, and obsolete change commentary.
Git retains source history.

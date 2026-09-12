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
| EMT | Clockwork incident correspondence and one-off agent interventions by email | [EMT](/Users/joey/rust/cell/emt/README.md) |
| Mentor | Daily problems and independent answer critiques | [Mentor](/Users/joey/rust/cell/mentor/README.md) |
| Paperboy | Reports from conversations or accepted Krisis decisions | [Paperboy](/Users/joey/rust/cell/paperboy/README.md) |
| Conversations | Local Codex task metadata and normalized message reads | [Conversations](/Users/joey/rust/cell/conversations/README.md) |
| Krisis | Decision identification, coverage, and delivery to Annals | [Krisis](/Users/joey/rust/cell/decisions/README.md) |
| Semantics | Registered project terminology and its revision history | [Semantics](/Users/joey/rust/cell/semantics/README.md) |
| Iatreion | Bounded read-only operational reports across declared Cell units | [Iatreion](/Users/joey/rust/cell/iatreion/README.md) |
| Usher | Declared Cell membership | [Usher](/Users/joey/rust/cell/usher/README.md) |
| Clockwork | Scheduled process activation and runtime history | [Clockwork](/Users/joey/rust/cell/clockwork/README.md) |
| Nucleus | Constrained agent execution, authentication, and job history | [Nucleus](/Users/joey/rust/cell/nucleus/README.md) |
| Chancery Usage | Registered systems/commands and append-only observed command invocations | [Usage journal](/Users/joey/rust/cell/chancery/provider/manuals/usage-record.md) |
| Annals Usage | Live Annals-attributed consumption and account allowance | [Usage reporting](/Users/joey/rust/cell/annals/docs/telemetry.md) |

## Topology and authority

```text
requesting products --> Nucleus --> isolated Codex app-server
       |                   |
       |                   +-- authentication, jobs, output, tool mailbox
       +-- domain records, validation, retries, and recovery

Clockwork --> registered product programs
Conversations --> normal-user Codex App Server
Krisis --> dedicated Annals decisions library --> Semantics, Conatus, Paperboy
Cast --> Platter <-- CRM career entries
Platter, Mentor, Paperboy, EMT --> Email --> Resend
installed product releases --> Chancery documentation
Cell declarations + product status probes --> Iatreion operational report
```

Conversations preserves empty message text and turns with no normalized messages.
Krisis requires nonblank user text in the selected exchange to identify user
decisions. Empty assistant text and earlier empty turns remain valid context.
Krisis and Paperboy embed Conversations, so normalization changes require
rebuilding and deploying those consumers as well as the standalone CLI.

On macOS, Conversations defaults to the ChatGPT app's bundled Codex at
`/Applications/ChatGPT.app/Contents/Resources/codex`. The CLI `--codex` option,
`CONVERSATIONS_CODEX`, and explicit library configuration can override that
selection. Consumer pins remain explicit. The app owns bundled Codex updates;
Conversations fails if its selected executable cannot start. Rebuild embedded
consumers to apply a changed library default.

Krisis uses `krisis/decision-document/1` for its sole active observer production
path. It freezes full normalized conversation through a completed exchange,
accepts a yes/no verdict and summary, and renders the source into Markdown.
The saved run protects request and result recovery; the observer commits
coverage and a target-bound document outbox. Local `document build` and
`document render` use the same builder without delivery or observer coverage.
Krisis marks an observation failed on its first processing error. A saved
conversation read failure returns zero and permits later scheduled work. Other
processing failures halt its Clockwork schedule. Explicit observation retry
preserves uncertain Nucleus request identity and pending Annals document identity.
`krisis health` reports worker
activity and its age; retained observation failures do not make the worker
unhealthy. See [Krisis source documents](/Users/joey/rust/cell/decisions/docs/source-documents.md).

Annals exchange contract 2 accepts the handed text without a decision schema
or source lookup. It owns ordinary storage metadata and returns complete accepted
documents through its dedicated feed. Paperboy reads that feed on demand and
reports documents selected by Annals acceptance time. Conatus forwards those exact bytes to its
library. Semantics supplies each post-activation document to each participating
project's reconciliation agent. Its `semantic-document-reconciliation/1` toolset
allows an empty result without creating a semantic revision. Interpretation and
connections belong to the agents and their instructions. Historical account
jobs retain their original immutable tool contracts. Persistent schemas are
Krisis 6, Annals 7, and Semantics 3; these migrations preserve accepted history
and cursors. Krisis refuses the document cutover while old handoffs or
classification jobs are in flight.

Conatus pins its Annals executable and its Clockwork definition pins Conatus.
Coordinated deployment rebinds those pins, verifies both existing library
identities, and preserves its cursor, operator pause and schedule intent.

Each product owns its data and success rules. Nucleus owns execution. A completed
model turn does not establish a product result. A later runtime failure does
not erase an already committed domain result.

Cast collection stores jobs without a title substring requirement. Configured
provider queries still limit discovery. Downstream consumers own selection by
title, seniority, location and other preferences.

Cast's `automatic_excluded_ats` defaults to Ashby for ordinary collection,
including existing configuration that omits the field. Platter's ad-hoc URL
path uses Cast collection contract 5 to retrieve one selected job despite that
exclusion or a disabled source. Cast preserves source enrollment and board-scan
state and retains only the selected posting within its ordinary budgets.

Platter `run-ad-hoc` uses Cast's exact `job collect` operation, then reuses
Platter's normal preparation, freshness, edition and send records. URL-selected
work creates no separate workflow state. Cast still owns the resulting source
and job records; Platter owns packet eligibility and delivery.

Platter runs resume drafting, independent Markdown review and revision as
separate sequential Nucleus jobs. It owns the handoffs and retained results.
The revision copies the retained writer setup and adds only the draft and
review. Review organization and editorial judgments are not parsed. Nucleus
continues to own execution and has no editorial workflow authority. Schema-three
Platter state preserves legacy preparations and prevents older binaries from
skipping the new review stage.

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

## Inspect Cell operational status

Use Iatreion when a question spans expected products, schedule admission,
current activity, local readiness, and the available evidence:

```sh
iatreion report /Users/joey/rust/cell
iatreion show annals/inbox --root /Users/joey/rust/cell --json
```

Iatreion reads the selected checkout, runs bounded product-owned
`status-snapshot --json` probes, joins explicit Clockwork binding facts, and
exits. It retains no report and does not initialize state, repair a product,
resume a schedule, refresh credentials, send mail, or run model work. Treat
intent, admission, activity, readiness, runtime outcome, and domain outcome as
separate fields. Missing or incompatible evidence remains unknown.

Use each row's inspection capability and record ID for deeper diagnosis. The
row does not authorize the referenced operation.

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
The deployment health client accepts a healthy service with open admission for
a requester-only update. When a hold exists, it requires that exact owner's sole
hold and complete drain. Ordinary health remains strict.

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

### Cell deployment

Preview the selected systems, then use the coordinator when deployment is authorized:

```sh
./deploy.sh plan SYSTEM...
./deploy.sh SYSTEM...
```

The coordinator selects a local `main` commit and prepares immutable candidates.
Product declarations order selected releases and identify affected installations
to hold. The plan adds missing or incompatible runtime dependencies and declared
installed companions, and reports each selection reason. Consumer-owned release
bounds select compatible candidates; sealed product inspectors prove retained
dependencies and their runtime prerequisites before maintenance.
Annals includes Usage; `decisions` aliases `krisis`.

The shared sequence is:

1. Prepare and verify all selected candidates before maintenance.
2. Hold and drain each affected consumer before its providers, so admitted work
   can finish using its dependencies.
3. When replacing Nucleus, hold it after requester continuation work has finished.
   A requester-only deployment leaves Nucleus admission open.
4. Prepare selected releases, then configure affected products in dependency order.
   Products retain their atomic state-and-file transactions. Nucleus starts its
   replacement service under its hold before requesters configure against it.
5. Verify selected candidate identity and affected-only installed readiness.
6. Release requester holds, then release Nucleus last when it is held.
7. Activate product schedules according to captured intent. Preserve existing
   pauses, disabled bindings and incident halts.

A hold belongs to one run, survives process exit, and does not expire. Releasing
it preserves other holds, operator pauses, and disabled schedules. Drain must
include durable unfinished work and associated Nucleus jobs.

Verification creates no model jobs or synthetic domain records. Recovery must
establish a coherent prior or candidate installation before releasing admission.
An uncertain apply is not repeated blindly. Matching files and health alone do
not prove replacement of a resident Nucleus daemon.

Setup settings supply missing choices once through `--settings ABSOLUTE_JSON`.
Product adapters reuse existing configuration, initialize missing state, update
pins and prepare disabled schedules. Email owns local credential installation
and receiving-account discovery; settings contain credential file references.
Clockwork refreshes enabled generated brokers during final activation while
preserving product intent, custom bindings and failure halts.

Unproved recovery retains the product hold and identifies its owner. A successful
recovery still reports the original deployment failure. Cleanup failure does not
erase installation success. The coordinator's final result distinguishes these
outcomes.

The coordinator retains no public deployment history or resume interface.
It retains an unresolved active transaction and uses it for recovery at the next
ordinary deployment command. It removes the workspace only after resolution.
Product holds and recovery backups remain until resolved. Release cleanup
preserves current releases and exact pins held by configuration, schedules, or
processes. Unknown or incomplete pin inventories stop deletion.
Cleanup reads configured pins through commands supported by retained products;
it uses Conatus `status` and excludes status history and diagnostics.
After an attempted release or activation, recovery holds and drains work again
before repairing product configuration. It uses the original captured intent.

See [Cell deployment](/Users/joey/rust/cell/deployment/README.md) for candidate,
locking, cleanup, and interrupted-operation details. Each product's installation
contract owns its state, migration, scheduler, and recovery procedure.

### Migration to coordinated deployment

An older executable may ignore a candidate's admission hold. Install a compatible
maintenance-capable release through the product's existing procedure first.
Capture enabled schedules and operator pauses before stopping admission.
Settle domain work and Nucleus jobs, install the compatible release, and verify
its maintenance interface. Restore only the captured enabled state after readiness.

Adapters invoke product-owned initialization and local credential setup from
supplied settings. External authentication still requires a valid supplied
account session. Domain imports use their explicit product operations. A
temporary maintenance pause does not replace the operator's original intent.

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

## EMT incident correspondence

EMT retains incident and exchange records with Nucleus job references. Agents
discover Cell operations through Chancery, investigate freely and author their
emails through emt send. Diagnosis is instructed to leave recovery changes for
the user's reply. Each recognized reply starts one bounded intervention.
Sender verification is explicitly deferred. Nucleus owns all execution and
tool activity; EMT has no operation ledger or product adapters.

EMT jobs use local execution and read-write workspace access. Their default
cwd is the user's home; Cell source is supplied separately. This permits
user-owned operational state and EMT mail writes, subject to Nucleus's actual
sandbox. The agent must observe its deadline and the affected product's
authority, maintenance and recovery contracts.

Clockwork's optional EMT preference defers a basic alert for 120 seconds.
EMT freezes its email before claiming initial-notification ownership. Claim
and basic-send admission are serialized. A claim does not expire or clear
the halt; EMT owns the delegated send outcome. A basic alert can precede a
late diagnostic follow-up. EMT's own failure uses Clockwork's basic path.

Refresh every active pinned Clockwork broker before enabling EMT preference.
Keep Clockwork's version-one notification-routing metadata with its incident
database during backup and recovery. Older brokers ignore claims. Read the
Clockwork and EMT installed contracts before cutover or rollback.

Hold and drain EMT before holding Nucleus during coordinated deployment.
Already admitted exchanges retain their job and email identities; installers
do not retry product work or clear halts. Disable emt/worker for program
replacement. Coordinated deployment restores its intended enabled selection
only after all holds are released and its broker uses the selected Clockwork.

## Codex weekly quota admission

`nucleus quota` and `GET /v1/quota` read the cached admission condition without
starting a model turn. Nucleus reads Codex App Server `account/rateLimits/read`
through its own credential authority every 60 seconds. It selects
`rateLimitsByLimitId.codex` and the single primary or secondary window whose
`windowDurationMins` is `10080`. It calculates remaining percent as
`100 - usedPercent`. It never substitutes the Spark bucket. Null, absent,
ambiguous, expired, or malformed weekly data is unknown, not zero or unlimited.
An explicitly identified legacy `rateLimits.limitId=codex` bucket is used only
when the map is absent. API-key authentication has no subscription weekly gate.

The default policy pauses new main-Codex work at 10% remaining or less. It
reopens only after a fresh observation exceeds 15%. An observation is usable
for at most 120 seconds and never past its reported reset. Failed reads can
use a still-fresh observation; otherwise admission pauses as `unknown`.
The reset time alone does not reopen admission. Quota is account-wide: use by
other CLI and desktop sessions can exhaust it between samples. The threshold
is a reserve, not a token reservation or a guarantee that active work finishes.

`quota-policy.json`, beside `nucleus.db`, configures the gate at daemon startup:

```json
{"enabled":true,"pauseAtRemainingPercent":10,"resumeAboveRemainingPercent":15}
```

Require `0 <= pause < resume < 100`. Keep this file and `quota-state.json` private
regular files with mode 0600. Invalid files fail startup. Nucleus writes the
state atomically. It contains the policy, account-identity digest, `limitId`,
`state`, remaining percentage, observation and reset times, and one condition ID
for a continuous pause. Times are Unix seconds. Missing numeric values remain
null. State and condition identity survive restart; account changes require a
new observation. Do not edit state to simulate recovery.

A rejected new submission returns HTTP 429 with code `quota_deferred` and the
quota snapshot in the response `details`. It creates no job or attempt. The Rust client
returns `ClientError::QuotaDeferred`. An exact replay of an admitted request
remains available. Accepted jobs recheck quota before execution and retain their
pending attempt while paused. They do not hold execution slots or start their
execution timeout while waiting. Job reads attach the quota condition to pending
main-Codex jobs. `get_job_for_work` yields a typed deferral for those jobs; raw
`get_job`, mailbox reads, cancellation, status and authentication remain available.
Started attempts drain. A structured Codex `usageLimitExceeded` becomes terminal
reason `quota_exhausted`; Nucleus pauses further admission immediately.

Requesters preserve pending work and immutable request identity on deferral.
Scheduled activations return success with an explicit quota outcome and do not
report an abend. Quota exhaustion after a start remains a retained failed attempt;
inspect domain effects before authorizing a retry. A committed domain result
remains authoritative. Existing deadlines and daily-report selection still apply:
expired work is not replayed automatically, and past Paperboy periods require
selection of their retained brief. Todo keeps its existing bounded wait once a
job is accepted. CRM retains its explicit resume operation and adds no scheduler.
Nucleus restart keeps its existing lost-attempt rule, including pending attempts;
a quota pause does not authorize replay across that boundary.

Health separates runtime readiness from quota admission: a healthy daemon can
report `status=ok`, `acceptingJobs=false`, and a blocked `quota`. Deployment holds,
operator pauses, and Clockwork failure halts are independent. Fresh quota recovery
releases only the quota condition; it clears none of those other controls.

EMT checks this condition in its existing worker. It freezes one deterministic
quota notice per condition ID and sends it directly through Email, without a
Nucleus invocation. Unknown quota has distinct wording. Notice identity and
transport progress survive restart under EMT's `quota-notifications/` directory.
At most two transport invocations use the same key and payload, five minutes
apart and within 23 hours. An unresolved send then remains uncertain and requires
inspection; it does not create a replacement message or model job. This prevents
per-service quota failure notices, but does not suppress unrelated incidents.

Upgrade all requester clients before enabling this gate on Nucleus. The new
clients tolerate a daemon without the optional quota health fields; EMT also
tolerates the old quota endpoint's 404. Use coordinated maintenance for cutover.
Keep the policy, state and EMT notice files with their private product backups.
No rollout, quota reset, or clearance of existing service halts is implicit.

## Shared command usage

Product CLI dispatches import `chancery-usage` and attempt one append to the
private Chancery journal. Each row records system and command identity, optional
`CODEX_THREAD_ID`, local insertion ID and whole Unix-second observation time.
The row observes handler entry; it establishes neither completion nor domain
success. Arguments, outputs, token counts and execution trees are absent.

After installing or updating a program, run its `--register-usage` mode. This
separate step initializes only an empty Chancery journal and idempotently adds
the full declared command inventory. Run both Annals programs. Source presence,
binary selection and catalog publication alone do not register commands.

Recording errors produce bounded diagnostics and preserve product results.
No automatic retry or migration runs. A missing thread remains unassociated;
services use the explicit request-scoped API rather than their startup
environment. Direct internal library calls are not automatically observed.
Email and Cast frontends preserve thread correlation through credential scrubbing.
CI disables dispatch recording; journal tests select isolated databases explicitly.

Use `chancery usage commands` or `chancery usage events` to read recorded
activity. The [usage contract](/Users/joey/rust/cell/chancery/provider/manuals/usage-record.md) owns schema, scope,
registration, privacy, compatibility and backup behavior.

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
- Read `chancery show nucleus.execution.operate` for service setup,
  authentication, backup, and restore. Use `chancery product nucleus` for its
  feature inventory and `chancery resolve ID` for required feature detail.

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
| Cast | Discovered companies, public jobs, collection outcomes, and exports | [Cast](/Users/joey/rust/cell/cast/chancery/overview.md) |
| Bazaar | Opaque strings and their immutable numbered versions | [Bazaar](/Users/joey/rust/cell/bazaar/chancery/overview.md) |
| Platter | Prepared job packets, frozen editions, and email outcomes | [Platter](/Users/joey/rust/cell/platter/README.md) |
| Clew | Append-only notes, named threads, optional external links, and explicit application reports with daily snapshots | [Clew](/Users/joey/rust/cell/clew/chancery/overview.md) |
| Annals | Retained sources, library instructions, interpretations, and corpus history | [Annals](/Users/joey/rust/cell/annals/README.md) |
| Conatus | Exact want intake, associations with accepted decisions, and deterministic daily email | [Conatus](/Users/joey/rust/cell/conatus/chancery/overview.md) |
| Email | Fixed-recipient submission and received-account mail reads | [Email](/Users/joey/rust/cell/email/chancery/overview.md) |
| EMT | Clockwork incident correspondence and one-off agent interventions by email | [EMT](/Users/joey/rust/cell/emt/chancery/overview.md) |
| Paperboy | Reports from conversations or accepted Krisis decisions | [Paperboy](/Users/joey/rust/cell/paperboy/README.md) |
| Weaver | Narratives authored from free-form directions and Annals reading | [Weaver](/Users/joey/rust/cell/weaver-narrative/README.md) |
| Conversations | Local Codex task metadata and normalized message reads | [Conversations](/Users/joey/rust/cell/conversations/chancery/overview.md) |
| Krisis | Decision identification, coverage, and delivery to Annals | [Krisis](/Users/joey/rust/cell/decisions/README.md) |
| Semantics | Registered project terminology and its revision history | [Semantics](/Users/joey/rust/cell/semantics/README.md) |
| Iatreion | Bounded read-only operational reports across declared Cell units | [Iatreion](/Users/joey/rust/cell/iatreion/README.md) |
| Usher | Declared Cell membership | [Usher](/Users/joey/rust/cell/usher/README.md) |
| Clockwork | Scheduled process activation and runtime history | [Clockwork](/Users/joey/rust/cell/clockwork/chancery/overview.md) |
| Nucleus | Constrained agent execution, authentication, and job history | [Nucleus](/Users/joey/rust/cell/nucleus/README.md) |
| Chancery | Installed product contracts, promise resolution, and recorded command usage | [Chancery](/Users/joey/rust/cell/chancery/provider/overview.md) |
| Annals Usage | Live Annals-attributed consumption and account allowance | [Usage reporting](/Users/joey/rust/cell/annals/docs/telemetry.md) |

## Topology and authority

```text
requesting products --> Nucleus --> isolated Codex app-server
       +-- Bazaar (exact selected prompt and tool-description versions)
       |                   |
       |                   +-- authentication, jobs, output, tool mailbox
       +-- domain records, validation, retries, and recovery

Clockwork --> registered product programs
Conversations --> normal-user Codex App Server
Krisis --> dedicated Annals decisions library --> Semantics, Conatus, Paperboy, Weaver
Cast --> Platter <-- Vita career works in Annals
Cast --> Clew application history
Platter, Clew, Conatus, Paperboy, EMT --> Email --> Resend
installed product releases --> Chancery documentation
Cell declarations + product status probes --> Iatreion operational report
```

Each product owns its data and success rules. Nucleus owns execution. A completed
model turn does not establish a product result. A later runtime failure does
not erase an already committed domain result.

Nucleus owns its private Codex credentials. Requesters do not copy or refresh
them. Conversations reads normal-user history through App Server. Annals Usage
reads Nucleus output and Annals attribution records without owning either store.

Krisis sends decision documents to Annals. Annals owns accepted text, library
identity, and the document feed. Paperboy selects documents by acceptance time;
Conatus forwards their exact text; Semantics reconciles them for participating
projects; Weaver reads them for authoring. Interpretation belongs to each
consumer. See [the document exchange](/Users/joey/rust/cell/annals/chancery/annals/manuals/decision-account-exchange.md)
and [Krisis source documents](/Users/joey/rust/cell/decisions/docs/source-documents.md).

Cast owns discovery and stored job records. Platter owns job selection, packet
preparation, and delivery. It reads career material from the Annals `vita`
library and obtains project prose from Weaver. Platter and Weaver retain their
own Nucleus requests. Platter cancellation and drain include its recorded Weaver
jobs, but exclude unrelated Weaver work. See
[Platter preparation](/Users/joey/rust/cell/platter/chancery/manuals/packet-prepare.md)
for collection, authoring, and acceptance rules.

Email owns transport and credential loading. Its receipt means provider
acceptance, not final inbox delivery. Conatus submits its deterministic daily
email through Email. The email path invokes no model.

Source colocation and a shared Cargo workspace do not merge product databases,
credentials, release units, or runtime authority.

## Scheduled failures

Clockwork owns activation, direct-process history, and the configured response
to an abend. Products own definition configuration, work, locks, idempotency,
logs, and domain recovery. A process exit does not establish product success.
Products report a failed scheduled operation even when its domain result was
already committed; the commit remains valid.

Schema-two definitions default to `halt-until-approved`. The first abnormal
scheduled attempt ends its current run. Clockwork retains a pending failure
episode and permits later activations while the shared service-health gate
checks for sustained failure. By default, five consecutive failed read-only
checks, at least 60 seconds apart, halt that binding and make its metadata-only
alert eligible together. These are health observations, not five product attempts.
A healthy check or explicit inactive intent, including operator pause, clears
a pending episode. Recovery after an established halt still requires approval.

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
| `paperboy/daily` | Explicit failed-brief retry and uncertain-send reconciliation. |
| `platter/daily` | Mark unavailable postings ineligible and continue with other candidates; preserve edition bytes and uncertain-send recovery. |
| `conatus/daily-email` | Preserve complete want wording, frozen email occurrences, and Email submission receipts; skip deliberate deployment holds. |
| `clew/daily-email` | Preserve complete application snapshots and frozen messages; require explicit recovery of uncertain submission. |
| `emt/worker` | Treat ordinary dependency unavailability as waiting; retain exchange identities and use the basic path for its own halt. |

Before migrating Clockwork runtime state, capture and disable existing bindings,
settle activations, and use its explicit backup-bearing migration. Old schema-one
definitions retain their historical policy until products generate and select
schema-two definitions. Preserve each captured enabled state and operator pause
when rebinding. Follow the Clockwork installation
contract for compatible binary/state recovery and notification retry limits.

## EMT incident correspondence

EMT retains incident and exchange records with Nucleus job references. Agents
discover Cell operations through Chancery and write diagnostic emails. EMT
instructs diagnosis agents to leave recovery changes for the user's reply.
This is an agent instruction, not a separate tool restriction. Each recognized
reply starts one bounded intervention under the affected product's authority
and recovery rules.
Nucleus owns execution; Email owns transport. See
[EMT incident response](/Users/joey/rust/cell/emt/chancery/manuals/incident-respond.md)
for reply recognition, permissions, deadlines, and retained records.

New EMT assignments use Nucleus invocation policy version two and require its
`workspace-unrestricted` capability. They use local execution with unrestricted
current-user filesystem, process, local socket, and network access; Codex
sandbox restrictions and approval prompts are disabled. Operating-system
permissions still apply. Deploy accepting Nucleus support before EMT emits
the new policy. Retained requests keep their original policy.

Clockwork gates new scheduling halts, basic alerts and EMT diagnosis on the
same consecutive failed read-only service checks. It reads a
bounded report from installed Iatreion. Healthy checks reset progress; explicit
inactive intent or operator pause excludes the service and resets progress.
Unknown health counts as failed with an explicit unknown condition. Historical
domain outcomes do not trigger this gate. Configure the threshold, interval and
stable Cell root with `clockwork notification policy`. Inspect pending episodes
and advance due checks with `clockwork notification check`, without mail or
product work. Inspect confirmed incident notification progress with
`clockwork notification show INCIDENT_ID`.

Existing broker visits and the EMT worker advance checks. No independent
daemon is added. A check can establish the halt at the threshold, but it does
not run or retry product work. Repeated failed checks in the same episode
create no new alert. A healthy or inactive observation clears a pending episode;
a resumed incident suppresses an unalerted episode. Existing halts, attempts
and claims retain their approval and delivery recovery rules.

EMT's five-minute diagnosis deadline starts at alert eligibility. Clockwork's
optional EMT preference then defers a basic alert for 120 seconds.
EMT freezes its email before claiming initial-notification ownership. Claim
and basic-send admission are serialized. A claim does not expire or clear
the halt; EMT owns the delegated send outcome. A basic alert can precede a
late diagnostic follow-up. EMT's own failure uses Clockwork's basic path.

Refresh every active pinned Clockwork broker before enabling EMT preference.
Keep Clockwork's `failure-checks.json`, `notification-routing.json` and
`notification-checks.json` with its incident database during backup and recovery.
Refresh every enabled pinned broker to the new failure-check contract. Older
brokers do not understand pending failure episodes and must not run while the
new sidecar exists. Preserve that state during recovery; do not roll back to an
older broker or discard it to bypass a pending episode. Read the Clockwork and
EMT installed contracts before cutover or rollback.

Hold and drain EMT before holding Nucleus during coordinated deployment.
Already admitted exchanges retain their job and email identities; installers
do not retry product work or clear halts. Disable emt/worker for program
replacement. Coordinated deployment restores its intended enabled selection
only after all holds are released and its broker uses the selected Clockwork.

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
Read `chancery show nucleus.service` for service status during quota pauses
and maintenance. Installation does not use health as a completion gate.

Do not maintain a dated installed-version table here. A source checkout or
catalog entry does not establish the currently running release.

## Codex weekly quota admission

Read `nucleus quota` to inspect admission without starting a model turn.
Requesters retain pending work and exact request identity on `quota_deferred`.
Scheduled deferral is an expected outcome, not a Clockwork abend. Preserve
existing deadlines and inspect domain effects before authorizing another
attempt after `quota_exhausted`. Quota recovery clears no deployment hold,
operator pause, or incident halt.

EMT owns the shared condition notice through Email without a model job.
Upgrade requester clients before enabling the gate and use coordinated
maintenance. Retain the quota policy, state, and EMT notices in their private
product backups. Read `chancery show nucleus.quota` for the complete Nucleus
policy, protocol, observation freshness, and recovery rules.

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

Rebuild embedded consumers when their provider's behavior changes. Krisis and
Paperboy embed Conversations, including its normalization and executable
defaults. Weaver embeds Annals, Nucleus, Iatreion, and Chancery usage interfaces.
Deployment companion declarations select these installed consumers for rebuild.
Keep explicit executable pins and library identities aligned during deployment.
See [Conversations installation](/Users/joey/rust/cell/conversations/chancery/manuals/installation-operate.md)
and each consumer's installation contract for the exact selection rules.

## Shared CI, release, and deployment coordination

Commit the intended changes, then submit that commit from the Cell root:

```sh
./ci.sh submit COMMIT
```

`cell-ci submit COMMIT` uses the same installed manager. The manager queues the
commit, integrates it privately, validates it, attempts bounded repairs, deploys
the accepted source, and emails the outcome. Root and product `ci.sh` wrappers
provide manager commands only. There is no direct check-only CI path.

The manager selects validation coverage from the fixed accepted base and each
committed candidate, including deletions and both paths of renames. Selective
success does not establish full repository validation. Source changes during
validation are stale. Linked worktrees share the CI broker and compiler
resources. See [CI submission](/Users/joey/rust/cell/ci_manager/README.md),
[validation selection](/Users/joey/rust/cell/pipeline/README.md), and
[the CI broker](/Users/joey/rust/cell/ci_broker/README.md).

Selected Rust tests run through one parallel nextest stage between the selected
products' checks before and after tests. The broker retains one compiler writer;
the test stage has a separate configurable test-process limit. Doctests and
shared infrastructure Python suites remain required separate commands. Install
the pinned runner on the external work volume before submitting a candidate
that uses it. An active job keeps its committed candidate. Updating validation
source does not replace the installed manager; manager replacement still requires
the maintenance procedure below.

Git publication remains separate. A product release command changes versions,
commits, tags, and pushes. CI makes private candidate commits and advances
accepted history; it does not publish remote Git refs. Release and deployment
preparation build and seal production artifacts; they do not rerun validation
or turn a build receipt into test evidence.

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
bounds select compatible candidates. Product installers read retained installation
metadata before maintenance.
Annals includes Usage; `decisions` aliases `krisis`.

Selected products use their supplied candidate for maintenance. Affected-only
products use their installed command. Admission holds and drain still apply.
Read each product's installation contract for its setup and recovery operations.

The shared sequence is:

1. Prepare all selected candidates before maintenance.
2. Hold and drain each affected consumer before its providers, so admitted work
   can finish using its dependencies.
3. When replacing Nucleus, hold it after requester continuation work has finished.
   A requester-only deployment leaves Nucleus admission open.
4. Prepare selected releases, then configure affected products in dependency order.
   Products retain their atomic state-and-file transactions. Nucleus starts its
   replacement service under its hold before requesters configure against it.
5. Release requester holds, then release Nucleus last when it is held.
6. Activate product schedules according to captured intent. Preserve existing
   pauses, disabled bindings and incident halts.

A hold belongs to one run, survives process exit, and does not expire. Releasing
it preserves other holds, operator pauses, and disabled schedules. Drain must
include durable unfinished work and associated Nucleus jobs.

Installation performs resource setup, program selection, and required state
initialization or migration. It does not run persistent-state integrity, artifact
integrity, or operational-readiness checks. CI does not assert those removed
installation checks. Ordinary product diagnostics and runtime guards remain.
Recovery restores the recorded prior or candidate setup before releasing
admission. An uncertain apply is not repeated blindly.

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

Annals recovery restores and checkpoints each journaled library backup before
declaring its installation safe. Copying progress does not consume its lock-wait
allowance. A blocked restore or checkpoint retains the deployment hold.

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
Settle domain work and Nucleus jobs, install the compatible release, and restore
only the captured enabled state after setup completes.

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

Conatus's daily-email timer does not invoke Nucleus. Its rendering reads
existing wants and Annals evidence. Preserve Krisis coverage and outbox state, Semantics cursors,
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

Read `chancery show nucleus.requester.integrate` for the integration procedure
and required checks. Use `chancery resolve nucleus.requester.integrate` to include
its required feature contracts. The `nucleus.invocation`,
`nucleus.requester-tools`, and `nucleus.output` pages own their public formats.

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

Pass `--json` when a caller parses CLI output. Bazaar, Cast, Clew, Clockwork,
Conatus, and Semantics use readable text by default. The flag selects each
product's structured output. Export files and internal installation interfaces
retain their documented formats.

Install the CI manager consumer that requests Bazaar `--json` before selecting
a Bazaar release with readable default output. Follow the paused, drained
manager replacement procedure. Update Cast collection callers with the matching
producer flag support during coordinated deployment. Cast installation uses the
native store interface so recovery can retain an older CLI.

## Serial CI delivery

Cell's CI manager, broker, compiler targets, release cache, and deployment
working state use one configured external APFS volume. CI bodies and release
compiler processes cannot write generated material to the host. Source Git
metadata, installed programs, configuration, and live product state retain
their existing owners. Provider runtime records remain provider-owned.
Read [external work storage](/Users/joey/rust/cell/ci_manager/STORAGE.md) before
storage cutover or cleanup. A missing drive stops work; it never selects a
fresh local queue. Resolve interrupted work before resetting any journal.

The installed `cell-ci` manager owns one durable FIFO queue for one configured
Cell Git common directory. Linked worktrees submit immutable commits to this
queue. Development continues on `main`. The manager owns `refs/ci/accepted`,
private input and candidate refs, and its private worktrees. `./ci.sh submit COMMIT`
and `cell-ci submit COMMIT` submit to this manager. Bare `./ci.sh` does not
validate; the manager invokes the internal validator for each candidate.

New submissions skip tests by default and retain other checks. Use
`./ci.sh submit COMMIT --run-tests` to run the selected tests. Existing jobs
retain their frozen test policy. Validation receipts and outcome emails state
when tests were skipped.

New macOS jobs freeze the persistent host signing policy and require verified
native production candidates before source acceptance, including when tests are
skipped. Builds, repair, installation and cache reuse preserve the selected
certificate and stable product identifiers. Configure or rotate signing only
with paused, settled CI and settled deployment and release operations. Read
`chancery show ci-manager.signing.operate` for setup, exact certificate selection,
key recovery and coverage; Python and standalone scripts retain their existing
runtime identities. Jobs without a signing snapshot keep their earlier path.

At dequeue, the manager records the current accepted commit as the job's base.
It merges the submitted commit into a private candidate. Each repair produces
a new commit before validation. Every validation compares the same accepted
base with the current candidate and retains aggregate gate receipts. Acceptance
uses a guarded ref update. Deployment selects that exact accepted candidate,
with a stable caller request ID and a retained operation receipt. Development
changes made after submission do not change the job.

Only one delivery lifecycle is active. The manager does not hold a CI broker
slot while it waits for a model, deployment, or email. Individual gates still
use the existing broker. A surviving model or deployment remains associated
with the active job after a manager restart. Unknown execution or lost-process
ownership blocks admission; a timeout does not prove termination.

CI is an ordinary Nucleus requester. It uses read-only workspace access and
built-in shell execution, with no requester tools or response schema. The
agent is asked to return only a raw Git patch in its final response. The manager
retains the exact response and adds a missing final LF to the application input.
Git applies it to a private parent index with `--cached --recount
--whitespace=nowarn`; the manager adds no patch acceptance rules. It commits the
result and runs the ordinary CI loop. New jobs freeze the refund policy: a
recorded private candidate refunds its repair attempt before the next validation
result. The default budget permits three unrefunded `gpt-5.6-terra` medium
attempts, then one unrefunded `gpt-5.6-sol` high attempt. Failed or rejected
attempts remain charged for the whole job. Accepted patches can exceed four
total invocations; no total-invocation ceiling applies. Retained jobs without
the refund flag keep their original limit on all invocations.

Attempt identities and history remain unique and complete after refunds. Job
status reports the budget mode, total, used, remaining, refunded, and total
invocations. Quota deferral preserves the same request identity without another
charge. Infrastructure failures do not select a stronger model. Bazaar supplies
the `cell.prompts.ci-manager` selection; import its components before activation.

Deployment holds the installed manager's Nucleus admission before replacing
Nucleus. CI reports drained when no admitted or unresolved model invocation
remains and the admission hold prevents another. Its delivery job can continue
to supervise deployment while that hold is present. Waiting for the delivery
job to finish at this boundary would cause a circular wait. The coordinator
releases only its own manager hold after coherent activation or recovery.

Email receives a retained program-authored outcome and idempotency key. The
manager permits two transport invocations, at least five minutes apart and
within 23 hours. Provider acceptance completes notification; uncertain delivery
blocks the queue. An email failure never restarts deployment. Validation,
acceptance, installation, cleanup, and notification retain separate outcomes.

Manager replacement uses explicit maintenance, outside its own delivery queue.
Pause admission and finish or recover the active job before replacement. The
installer selects a fixed release and compatible journal schema under exclusive
ownership. It preserves queued jobs and starts the replacement paused. It does
not load worker code from mutable development source or clear existing holds.
The CI manager is shared infrastructure; the product deployment inventory does
not deploy the manager itself.

Use the [CI operation contract](/Users/joey/rust/cell/ci_manager/chancery/manuals/queue-operate.md)
for initialization, controls, Git patch application, retained evidence, and
recovery outcomes. Installation and queue activation are separate operations.

## Shared command usage

Product CLIs record agent command invocations in Chancery's private usage
journal. New rows require thread attribution. They contain command identity
and observation time, but no arguments or output. A row does not prove success.

After installing or updating a program, run its `--register-usage` mode. This
separate step initializes only an empty Chancery journal and idempotently adds
the full declared command inventory. Run both Annals programs. Source presence,
binary selection and catalog publication alone do not register commands.

Recording errors preserve product results. Product dependency processes and
hooks mark internal calls with `CHANCERY_USAGE_INTERNAL=1`; wrappers preserve
that marker and thread attribution. Nucleus clears both for each new agent,
which supplies its own attribution. Rebuild participating binaries and update
their wrappers, hooks, and pinned brokers when these rules change.

Use `chancery usage commands` or `chancery usage events` to read recorded
activity. The [usage contract](/Users/joey/rust/cell/chancery/provider/manuals/usage-record.md) owns schema, scope,
registration, privacy, compatibility and backup behavior.

## Guarded change playbooks

Identify the owning product before changing a boundary. Update its behavior,
contracts, and affected integrations together. Update this manual only when
shared facts or procedures change.

### Routine Nucleus patch

1. Identify affected public meaning, schema, harness, and requester obligations.
2. Update the owning code and documentation, commit the changes, and submit
   them through the CI manager. Verify its validation and deployment outcome.
3. Publish only when authorized. Release requires clean `main` synchronized
   with `origin/main` and creates the commit and tag.
4. Read the retained manager deployment result and report requester admission.
   Installation does not establish operational readiness.
   For a separate manual installation or recovery, quiesce affected work and
   deploy matching CLI and daemon candidates before restoring admission.

Read `chancery show nucleus.execution.operate` for the exact installer and
rollback procedure.

### Exact Codex upgrade

Inspect the candidate executable, version, model catalog, app-server schema,
and every consumed method and isolation rule. Update the adapter and its
compatibility checks before deployment. After cutover, health must identify the
exact executable, supported version, and required capabilities.

The adapter requires Codex `0.154.0-alpha.6.2`. Stage its complete runtime with
`<TESTED_NUCLEUS_INSTALL> stage-harness --codex /absolute/release/codex` before
CI submission. The source must include the matching `codex-code-mode-host`.
The installer records both file identities and publishes the complete runtime
under `~/Library/Application Support/Nucleus/harnesses/codex/VERSION/runtime/`.
An existing staged directory is reused from its recorded manifest. See
`chancery show nucleus.execution.operate` for the installation procedure and
`chancery show nucleus.service` for source and recovery guarantees.

Automated tests cover in-memory behavior. They do not execute the Codex runtime,
launch a daemon, bind sockets, or create filesystem fixtures. Runtime execution
and filesystem recovery are outside automated test coverage.

Installation copies and selects the configured runtime without using health or
artifact-integrity checks as an installation gate. Ordinary runtime admission
continues to enforce its harness and authentication requirements. A runtime
upgrade does not clear requester failure halts or authorize retries.

### Public protocol or client change

For additive support, deploy the accepting daemon before new callers. For an
incompatible change, retain both forms during migration or quiesce all affected
requesters. Update core types, client, daemon routes, owning feature contracts,
examples, and contract checks together.

### Nucleus database schema change

Provide a versioned migration from every supported prior version. Define the
transaction boundary, post-commit maintenance, backup, and rollback procedure.
Before cutover, settle requesters and pending calls, stop Nucleus, and take the
required consistent backup. Validate retained jobs, output, and mailbox integrity.

Old binaries must not open an incompatible new database. Recovery across that
boundary requires a matching database and binary pair. Read
`chancery show nucleus.service` for schema recovery guarantees and
`chancery show nucleus.execution.operate` for the restoration procedure.

### Requester schema, toolset, prompt, or permission change

Publish new immutable identities when registered meaning changes. Keep historical
decoders. The requester owns prompt, model, reasoning, timeout, workspace, tools,
and launch-context policy. Use new job IDs for new attempts and check the
required Nucleus capabilities and domain behavior.

Cell requesters retrieve authored text through Bazaar. Each `cell.prompts.OWNER`
string pins exact component versions. Publish components before the complete
selection; resolve that set before admission. Keep exact requests or selection
versions for recovery. Nucleus receives the resolved request and does not read
Bazaar. Library instruction revisions and assignment captures remain domain
snapshots under their existing retention rules.

Import the reviewed prompt seed before deploying these callers. Runtime reads
use `~/.local/share/bazaar/bazaar.sqlite3`, or an absolute `CELL_BAZAAR_DATABASE`
override. They fail when required state is missing and never initialize it.
An interactive override does not change a scheduled process environment. Keep
the migration selection and historical component versions. See the
[prompt migration procedure](/Users/joey/rust/cell/prompting/README.md).

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

After a shared change, read the retained installation outcome. Use product
diagnostics separately when operational evidence is needed. Release only holds
and pauses owned by the operation. Preserve pre-existing disabled schedules.

## Where facts and changes belong

Keep each full explanation with its owning product. This manual owns shared
topology, authority, compatibility, coordination, and recovery order. Product
references own record meaning and exact operations.

Nucleus, Annals, Annals Usage, Bazaar, Cast, Chancery, Clew, Clockwork, Conatus,
Conversations, Email, EMT, Platter, Semantics, Usher, and Weaver publish detailed
behavior as feature contracts in their product-owned Chancery provider bundles.
Read `chancery product PROVIDER_ID`
for the overview and inventory, `chancery show ID` for one feature or procedure,
and `chancery resolve ID` for the complete required contract reading.
Operation manuals keep prerequisites,
action-critical effects, stop conditions, and verification in place. Required
dependencies declare compatible contracts; related references provide navigation.
The remaining products keep their current documentation organization.

Use short active sentences and descriptive headings. State which records or
operation a count, timestamp, or failure describes. Remove duplicate explanations,
completed-work narratives, test-result reports, and obsolete change commentary.
Git retains source history.

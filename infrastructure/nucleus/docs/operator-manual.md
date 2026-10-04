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
  authentication and service recovery. Use `chancery product nucleus` for its
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
| Milieu | Retained companies, jobs, source metadata, and exports | [Milieu](/Users/joey/rust/cell/products/milieu/chancery/overview.md) |
| Bazaar | Versioned strings, shared prompt selection, rendering, and explicit imports | [Bazaar](/Users/joey/rust/cell/infrastructure/bazaar/chancery/overview.md) |
| Platter | Prepared job packets, frozen editions, and email outcomes | [Platter](/Users/joey/rust/cell/products/platter/README.md) |
| Clew | Append-only notes, named threads, optional external links, and explicit application reports with daily snapshots | [Clew](/Users/joey/rust/cell/products/clew/chancery/overview.md) |
| Annals | Retained sources, library instructions, interpretations, and corpus history | [Annals](/Users/joey/rust/cell/products/annals/README.md) |
| Conatus | Exact want intake, associations with accepted decisions, and deterministic daily email | [Conatus](/Users/joey/rust/cell/products/conatus/chancery/overview.md) |
| Email | Fixed-recipient submission and received-account mail reads | [Email](/Users/joey/rust/cell/infrastructure/email/chancery/overview.md) |
| EMT | Clockwork incident correspondence and one-off agent interventions by email | [EMT](/Users/joey/rust/cell/infrastructure/emt/chancery/overview.md) |
| Paperboy | Manifest renderer execution and exact stdout email handoff | [Paperboy](/Users/joey/rust/cell/products/paperboy/chancery/overview.md) |
| Weaver | Narratives authored from free-form directions and Annals reading | [Weaver](/Users/joey/rust/cell/products/weaver-narrative/README.md) |
| Conversations | Local Codex task metadata and normalized message reads | [Conversations](/Users/joey/rust/cell/products/conversations/chancery/overview.md) |
| Krisis | Decision identification, coverage, and delivery to Annals | [Krisis](/Users/joey/rust/cell/products/decisions/README.md) |
| Semantics | Registered project terminology and its revision history | [Semantics](/Users/joey/rust/cell/infrastructure/semantics/README.md) |
| Iatreion | Bounded read-only operational reports across declared Cell units | [Iatreion](/Users/joey/rust/cell/infrastructure/iatreion/README.md) |
| Usher | Declared Cell membership | [Usher](/Users/joey/rust/cell/infrastructure/usher/README.md) |
| Clockwork | Scheduled process activation and runtime history | [Clockwork](/Users/joey/rust/cell/infrastructure/clockwork/chancery/overview.md) |
| Nucleus | Constrained agent execution, authentication, and job history | [Nucleus](/Users/joey/rust/cell/infrastructure/nucleus/README.md) |
| Chancery | Installed product contracts, promise resolution, and recorded command usage | [Chancery](/Users/joey/rust/cell/infrastructure/chancery/provider/overview.md) |
| Annals Usage | Live Annals-attributed consumption and account allowance | [Usage reporting](/Users/joey/rust/cell/products/annals/docs/telemetry.md) |

## Topology and authority

```text
requesting products --> Nucleus --> isolated Codex app-server
       +-- Bazaar (exact selected prompt and tool-description versions)
       |                   |
       |                   +-- authentication, jobs, output, tool mailbox
       +-- domain records, validation, retries, and recovery

Clockwork --> registered product programs
Conversations --> normal-user Codex App Server
Krisis --> dedicated Annals decisions library --> Semantics, Conatus, Weaver
Milieu --> Platter <-- Vita career works in Annals
Milieu --> Clew application history
Platter, Clew, Conatus, Paperboy, EMT --> Email --> Resend
configured renderer scripts --> Paperboy --> Email
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
identity, and the document feed. Conatus forwards their exact text; Semantics
reconciles them for participating projects; Weaver reads them for authoring.
Interpretation belongs to each
consumer. See [the document exchange](/Users/joey/rust/cell/products/annals/chancery/annals/manuals/decision-account-exchange.md)
and [Krisis source documents](/Users/joey/rust/cell/products/decisions/docs/source-documents.md).

Milieu owns stored job records and the retained read handoff. Milieu performs no
collection. Platter owns job selection, packet
preparation, and delivery. It reads career material from the Annals `vita`
library and obtains project prose from Weaver. Platter and Weaver retain their
own Nucleus requests. Platter cancellation and drain include its recorded Weaver
jobs, but exclude unrelated Weaver work. See
[Platter preparation](/Users/joey/rust/cell/products/platter/chancery/manuals/packet-prepare.md)
for source capture, authoring, and acceptance rules.

Email owns transport and credential loading. Its receipt means provider
acceptance, not final inbox delivery. Conatus submits its deterministic daily
email through Email. The email path invokes no model.

Paperboy executes manifest commands and sends successful nonempty stdout
unchanged through Email. Its scripts own collection, reporting windows, and
source integrations. Paperboy itself uses Clockwork and Email; it retains no
report or send ledger and invokes no Nucleus agent.

Source colocation and a shared Cargo workspace do not merge product databases,
credentials, release units, or runtime authority.

Cell groups infrastructure source in `infrastructure/` and application source
in `products/`. The root Cargo workspace and lockfile include both groups.
Product IDs, release units, and deployment selection remain independent of these
directory groups.

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
explicit maintenance holds and declared readiness waits are expected product outcomes;
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
| `paperboy/JOB_ID` | Execute the applied renderer snapshot; skip empty stdout and fail without sending invalid output. No retained payload or retry queue. |
| `platter/daily` | Mark unavailable postings ineligible and continue with other candidates; preserve edition bytes and uncertain-send recovery. |
| `conatus/daily-email` | Preserve complete want wording, frozen email occurrences, and Email submission receipts; skip explicit maintenance holds. |
| `clew/daily-email` | Preserve complete application snapshots and frozen messages; require explicit recovery of uncertain submission. |
| `emt/worker` | Treat ordinary dependency unavailability as waiting; retain exchange identities and use the basic path for its own halt. |

Before migrating Clockwork runtime state, capture and disable existing bindings,
settle activations, and use its explicit migration. Old schema-one
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
[EMT incident response](/Users/joey/rust/cell/infrastructure/emt/chancery/manuals/incident-respond.md)
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
`notification-checks.json` with its incident database during maintenance and recovery.
Refresh every enabled pinned broker to the new failure-check contract. Older
brokers do not understand pending failure episodes and must not run while the
new sidecar exists. Preserve that state during recovery; do not roll back to an
older broker or discard it to bypass a pending episode. Read the Clockwork and
EMT installed contracts before cutover or rollback.

Use explicit EMT and Nucleus maintenance operations when the intended operation
requires their admitted work to finish. Deployment does not discover this order.
Already admitted exchanges retain their job and email identities; installers
do not retry product work or clear halts. Disable emt/worker explicitly when admitted work must finish before replacement.
Product scheduling instructions preserve the intended enabled selection and
replace its executable pins; the executor infers no group maintenance policy.

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
product state. Read `chancery show nucleus.quota` for the complete Nucleus
policy, protocol, observation freshness, and recovery rules.

## Compatibility model

| Boundary | Required treatment |
| --- | --- |
| CLI and daemon release | Install matching candidates. Inspect both with health and version reads. |
| Public invocation protocol | Deploy additive daemon support before requesters emit it. Use a new protocol for incompatible meaning. |
| Capability contract | Version incompatible documented behavior and review consumer dependency bounds. |
| Codex harness | Prove the exact adapter version before replacing the configured executable. |
| Persistent schema | Migrate transactionally and select a program that supports the current schema. |
| Immutable schemas and toolsets | Give changed meaning a new identity. Keep decoders for retained jobs. |
| Requester client | Rebuild when consumed types or behavior change. Shared source does not require lockstep deployment. |

If old and new protocol forms cannot coexist, stop admission and settle all
affected work before the coordinated cutover. Credential recovery stays separate
from program and database rollback. Never restore an older credential as a side
effect of either rollback.

A decoder repair may expose output from an old completed job without another
model attempt. Missing observations remain a gap. The repair does not change
requester terminal records or authorize a retry.

Rebuild embedded consumers when their provider's behavior changes. Krisis
embeds Conversations, including its normalization and executable defaults.
Weaver embeds Annals, Nucleus, Iatreion, and Chancery usage interfaces.
Select affected consumers explicitly when their supported interfaces change.
Keep executable pins and library identities aligned through product instructions.
See [Conversations installation](/Users/joey/rust/cell/products/conversations/chancery/manuals/installation-operate.md)
and each consumer's installation contract for the exact selection rules.

## Shared CI, release, and deployment coordination

Commit the intended changes, then submit that commit from the Cell root:

```sh
./ci.sh submit COMMIT
```

`telete submit COMMIT` uses the same installed program. Root and product
`ci.sh` wrappers select Telete. It queues the commit, integrates it privately,
validates it, applies bounded repairs, prepares signed production candidates,
deploys the exact selected source, and emails the outcome. There is no direct
check-only CI path.

Telete compares each candidate with the accepted base captured when the job
is claimed. Selection includes deletions and both rename paths. A selective
pass does not establish full repository coverage. Read
[Telete queue operation](/Users/joey/rust/cell/infrastructure/telete/chancery/manuals/queue-operate.md)
for selection, provider authority, receipts, and recovery.

New jobs skip tests by default. Use `--run-tests` to run selected Rust tests,
and prepare the pinned runner with `telete prepare-tools` first. Native
structure, provider, shell, formatting, lint, build, and signing checks remain.
Telete does not invoke the Python CI helpers or regression runner.

Telete compiles selected production packages together before acceptance and
signs declared native commands with the frozen host identity. Deployment
uses exactly the selected manifests and retained candidates. Instruction
completion does not establish product health. Compilation failures can use
repair; signing and other preparation failures stop the job.

Git publication remains separate. A product release command changes versions,
commits, tags, and pushes. CI makes private candidate commits and advances
accepted history; it does not publish remote Git refs. Release and deployment
preparation build and seal production artifacts; they do not rerun validation
or turn a build receipt into test evidence.

### Cell deployment

Preview the instructions, then execute an authorized deployment:

```sh
./deploy.sh plan SYSTEM...
./deploy.sh SYSTEM...
```

The executor selects the exact supplied products and source commit. Annals
includes Usage; `decisions` aliases `krisis`. Each product declares an ordered
manifest of `run`, `copy`, and `link` operations. The executor does not add
products from installed versions, runtime dependencies, or requester discovery.
Select every product required by the intended change explicitly.

The executor builds or reuses the requested packages, then carries out each
manifest instruction in its declared order. A `run` instruction supplies an
executable, arguments, environment, input and timeout. Its output is opaque.
A zero exit code completes that instruction. File operations complete when the
requested copy or atomic link replacement succeeds.

Product installation commands own their declared program placement, state setup,
configuration, runtime pins, schedules, and service operations. Supply missing
choices through `--settings ABSOLUTE_JSON`. Settings contain credential file
references rather than credential bytes. Product runtime guards and diagnostics
retain their own contracts.

Deployment success means every instruction completed. It does not establish
application health, integrity, compatibility, trust, or requester admission.
A failed instruction retains earlier effects. An interrupted instruction has
unknown effects. The executor does not retry instructions or infer a product
rollback, maintenance release, or recovery action.

One host lock serializes execution and remains inherited by an executing child.
The private receipt retains the supplied manifest, source and caller identities,
step start and completion, child identity, timing, and log references. After an
interruption, inspect those records and use the product's explicit procedures as
needed. Acknowledge the interrupted executor only after its process has stopped;
acknowledgement releases executor admission and makes no application recovery
claim. Old unfinished lifecycle runs cannot resume under this executor.

Cleanup removes executor scratch. It does not inventory installed configuration,
schedule pins, or running applications and does not delete installed releases.
See [Cell deployment](/Users/joey/rust/cell/deployment/README.md) for the manifest
schema, command interfaces, locking, and retained results.

## Quiesce before work that cannot tolerate a lost attempt

For an attended service, storage, or authentication operation that must preserve
admitted work:

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

Pass `--json` when a caller parses CLI output. Bazaar, Milieu, Clew, Clockwork,
Conatus, and Semantics use readable text by default. The flag selects each
product's structured output. Export files and internal installation interfaces
retain their documented formats.

The retained Python CI manager must request Bazaar `--json` when parsing
readable-default releases. Telete uses the supported Rust reader. Milieu installation uses the
native store interface so recovery can retain an older CLI.

## Serial CI delivery

Telete owns Cell's active serial delivery queue, gate broker, private worktrees,
and deployment records. Root and product `ci.sh` wrappers select installed
Telete. Development remains on `main`; Telete owns `refs/telete/accepted` and
`refs/telete/jobs/`. Its CLI and embedded provider come from one fixed installed
release rather than the changing checkout.

The configured external APFS workspace holds Telete state, compiler targets,
Cargo downloads, caches, and temporary files. Host storage and signing settings
remain shared Cell selections. Keychain owns the private signing key. Drive
loss stops work and never selects a new queue or internal-disk fallback.

At claim, Telete freezes the accepted base and creates a private merge candidate.
Repairs create private child commits, which are validated against that same
base. Promotion uses an expected-old ref update after validation and required
production preparation. Acceptance remains recorded if later deployment fails.

Nucleus owns repair execution and credentials. Telete retains exact requests
and provider identities before admission. Bazaar supplies the dedicated
`cell.prompts.telete` selection. Publish its component versions before the
complete selection. Only the model's final response supplies the raw Git patch.
Git decides whether it applies. A recorded candidate refunds the repair budget
point; rejected or failed attempts stay charged. Refunds preserve invocation
history and provide no total-invocation ceiling.

Deployment performs the exact selected product manifests in order. Products
own their program selection, state, configuration, schedules, and recovery.
Interrupted effects remain uncertain and require the retained recovery
procedure. Notification has its own frozen text, Email identity, and outcome.
Email acceptance means provider submission rather than inbox delivery.

Production candidates and installed releases use opaque UUID identities. Cell
compares normalized signing-policy objects directly and lets Cargo manage build
reuse. Build preparation, installation, and program rollback compute no custom
artifact hashes. Packaging invokes native signing with the configured certificate
and permanent identifiers; deployment does not audit the signatures.
Existing hash-named releases remain readable; their recorded hashes are ignored.
Native product installers publish actual payload files at each product's fixed
`install/runtime` path. Public executable links resolve to that path. Updates
and program recovery replace regular files there; a UUID change does not move
the executable. Nucleus keeps its existing fixed public CLI and daemon files.
The `current` and `previous` links select retained release archives, and
provider directory links continue to select a coherent archived bundle.
Product lifecycle operations must quiesce affected execution before replacing
runtime files. Each file replacement is atomic; the complete tree is not.
Recovery still requires a program compatible with the product's state and fixed
runtime layout. Retained pre-runtime binaries and definitions keep their legacy
limits; rebuild reviewed historical source with the current installation
protocol when those limits prevent recovery.

Clockwork runtime launch pins and product data-migration checks retain their
separate protocols. New release formats require a current trusted installer for
recovery; older installers cannot read them.
For Telete replacement, pause and settle its work, stop its worker, and run
`telete install` from the intended fixed executable. Installation preserves
state and leaves the service stopped. Start the service and resume admission
explicitly. For Nucleus maintenance, quiesce Telete through its requester
maintenance controls and preserve exact hold ownership. Telete does not operate
or recover other requesters.

The retained Python manager remains available as `cell-ci` for its own records.
Keep its queue paused and service stopped while Telete owns new work. Preserve
`refs/ci/accepted`, job refs, journal, and receipts. Telete has no journal or
request-key import. Initial handoff selects an exact accepted Git commit;
initialization does not validate that source. Do not rewind either accepted
history or bypass unresolved operations with another state directory.

Read [Telete queue operation](/Users/joey/rust/cell/infrastructure/telete/chancery/manuals/queue-operate.md)
for current controls and outcomes. Read the
[retained Python CI contract](/Users/joey/rust/cell/ci_manager/chancery/manuals/queue-operate.md)
for old job inspection and recovery. Manual release and deployment tooling
retain their separate interfaces and Python requirements.

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
activity. The [usage contract](/Users/joey/rust/cell/infrastructure/chancery/provider/manuals/usage-record.md) owns schema, scope,
registration, privacy and compatibility.

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
transaction boundary, post-commit maintenance, and program/schema compatibility.
Before cutover, settle requesters and pending calls and stop Nucleus. Validate
retained jobs, output, and mailbox integrity.

Old binaries must not open an incompatible new database. Recovery across that
boundary requires a program that supports the current database schema. Read
`chancery show nucleus.service` for schema recovery guarantees and
`chancery show nucleus.execution.operate` for the service recovery procedure.

### Requester schema, toolset, prompt, or permission change

Publish new immutable identities when registered meaning changes. Keep historical
decoders. The requester owns prompt, model, reasoning, timeout, workspace, tools,
and launch-context policy. Use new job IDs for new attempts and check the
required Nucleus capabilities and domain behavior.

Bazaar owns shared prompt selection, rendering, trusted reference expansion,
description preparation, and explicit imports. Each `cell.prompts.OWNER` string
pins exact component versions. Publish components before the complete selection;
resolve that set before admission. Products assemble typed Nucleus requests and
toolsets, choose models and permissions, and retain exact requests or selection
versions for recovery. Bazaar has no Nucleus dependency. Nucleus receives the
resolved request and does not read Bazaar. Library instruction revisions and
assignment captures remain domain snapshots under their existing retention rules.

Import the reviewed prompt seed before deploying these callers. Runtime reads
use `~/.local/share/bazaar/bazaar.sqlite3`, or an absolute `CELL_BAZAAR_DATABASE`
override. They fail when required state is missing and never initialize it.
An interactive override does not change a scheduled process environment. Keep
the migration selection and historical component versions. Read
`chancery show bazaar.prompts.prepare` and `chancery show bazaar.prompts.import`
for the owning feature contracts.

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
| Unresolved explicit maintenance hold | Use that product's retained recovery procedure. |

After a shared change, read the retained installation outcome. Use product
diagnostics separately when operational evidence is needed. Release only holds
and pauses only when the operation explicitly acquired them. Preserve pre-existing
disabled schedules.

## Where facts and changes belong

Keep each full explanation with its owning product. This manual owns shared
topology, authority, compatibility, coordination, and recovery order. Product
references own record meaning and exact operations.

Nucleus, Annals, Annals Usage, Bazaar, Milieu, Chancery, Clew, Clockwork, Conatus,
Conversations, Email, EMT, Paperboy, Platter, Semantics, Usher, and Weaver publish
detailed behavior as feature contracts in their product-owned Chancery provider bundles.
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

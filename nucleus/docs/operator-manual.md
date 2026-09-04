# Nucleus ecosystem operator manual

This manual is the starting point for operating Nucleus, changing a boundary
shared by Nucleus and its requesters, or adding a new requester. It describes
current operational truth and safe change ordering. It is not a backlog,
changelog, protocol specification, global capability directory, or substitute
for a requester's domain documentation.

The checked-in file is canonical. An installed, version-matched Nucleus CLI
prints the same Markdown without contacting the daemon:

```sh
nucleus manual
```

For interactive terminal reading, use `nucleus manual | less`.

The standard installation described here is the current user's local macOS
installation. The stable invocation contract can support another packaging
topology, but that topology must define its own service, state, backup, and
credential procedures.

## Choose the system by the intended outcome

Do not route by isolated words such as “save”, “remember”, “job”, or “agent”.
First identify the durable outcome the user wants. If no durable outcome below
is intended, perform the work normally. If the user explicitly requests more
than one outcome, use each relevant system.

The installed Chancery directory is the global entry point for this decision.
List its complete installed catalog, compare the intended outcome semantically
with the entries' titles and summaries, then read every plausible
version-matched contract before choosing or invoking an interface:

```sh
chancery list
chancery show ENTRY_ID
```

When the question concerns a system's complete outward promise or a proposed
design reliance, resolve the selected exact ID after semantic discovery:

```sh
chancery resolve ENTRY_ID
```

Resolution assembles the owner-declared provider scope, normalized boundary
facets, complete root and transitive documentation contracts, exact basis, and
explicit gaps. It is exact-ID analysis, not natural-language matching. Treat
unsupported, unspecified, not-applicable, and undeclared facts distinctly;
never fill a promise gap from a database schema or implementation detail.

Chancery is a read-only documentation and discovery authority. It does not run
the capability, choose an entry, probe readiness, authorize an effect, or
determine domain success. The interactive agent owns semantic selection. The
table below remains a compact explanation of Nucleus ecosystem authority, not a
separately maintained discovery catalog.

| System | Use it when | It owns | Do not use it for |
| --- | --- | --- | --- |
| Todo | An actionable concern or follow-up should be researched and retained for later. | Concern provenance, routing and its explicit decisions, stable todo identities, dated situation assessments, proposed or accepted designs, open/done state, and working notes. | Work requested for immediate completion, general knowledge, implementation execution, or shared runtime policy. |
| CRM | Employment-relevant people, opportunities, and contemplated contact should be retained as evidence-grounded cases. | Its local SQLite library, queued steward runs, immutable case revisions, evidence, and advisory review notes. | Sending or authorizing outreach, scheduled intake, treating an advisory as a gate, or storing CRM domain state in Nucleus. |
| Annals | Immutable source material should be retained or reconciled with an evidence-grounded conceptual corpus, or that corpus should be searched or explored. | Each selected physical library's retained works, concepts, evidence, reconciliations, revisions, source deliveries, inbox policy, and domain recovery. | An action backlog, casual notes or preferences, agent-process supervision, cross-library federation, or account telemetry. |
| Weaver | Authored repository inputs should become the current five-stage public-facing narrative outputs. | Current-run admission, stage order and input snapshots, repository output writes, validation, cancellation intent, and recovery. | Publishing, editing a public profile, treating generated text as factual authority, or general job orchestration. |
| Email | A plain-text email should be sent to the single fixed recipient. | The synchronous Resend request and its fixed sender and recipient contract. | Drafting without sending, arbitrary recipients, or agent execution. |
| Conversations | Codex tasks on this Mac should be listed, inspected, or searched. | A read-only normalized view over the normal user's Codex App Server. | Decision classification, durable projections, live-process supervision, or Nucleus's isolated job history. |
| Krisis | Attributable decisions in completed root user turns should be identified and delivered as immutable accounts to the dedicated Annals decisions library. | The observation baseline and coverage, bounded classification, source anchors, account projection, durable outbox, Annals acceptance receipts, and recovery. | Judging truth, importance, applicability, enactment, current force, review state, supersession, retaining the canonical account library, or sending a digest. |
| Semantics | A registered project folder's authoritative terminology and semantic history should be explored or maintained from accepted accounts in the dedicated Annals decisions library. | Project registration and routing, stable concept identities, append-only semantic revisions and evidence, decision-feed intake, Nucleus reconciliation, and recovery. | General documentation generation, unregistered folders, source-code behavior, transcript storage, or interpreting Annals retention as semantic truth. |
| Geste | A prior bounded work episode should be found by problem shape or manually recorded with its source basis. | Episode identity, immutable account revisions, authored interpretation, source anchors, coverage gaps, and read-time search, report, and graph projections. | Source-system truth, current policy, automatic episode ingestion, or deciding that a precedent applies. |
| Pratica | A proposed entrant needs exact negotiated terms from several independently stewarded systems. | Integration and track identity, immutable offers, current assent, agreement seals, steward bases, caller-keyed ingress receipts, bounded requester attempts, composition reviews, and conformance reviews. | Implementing or changing target systems, automatically discovering every concern, treating review prose as assent, or proving deployment readiness. |
| Nucleus | A local application needs constrained agent execution, or shared execution, authentication, compatibility, job history, deployment, or requester integration must change. | Admission, the portable invocation contract, eight-slot harness supervision, single-authority credential coordination, cancellation, exact harness-stdout observations, and the durable dynamic-tool mailbox. | Domain success, project registration, workflow graphs, requester retry policy, or reporting materializations. |
| Annals Usage | Annals-attributed model consumption, account allowance, login, or the Annals-to-Nucleus execution path must be inspected. | Live calculation over Annals attribution and Nucleus output atoms, plus Annals-facing budget and diagnostic commands. | Nucleus runtime authority, durable reporting projections, Codex credential storage, Annals corpus success, or general job orchestration. |
| Codex | Nucleus needs an inspected harness and account protocol implementation. | Its executable and app-server behavior. | Requester domain policy or a second credential authority for Nucleus jobs. |

Typical routing examples:

- “Leave this concern for later” is Todo work.
- “Retain this possible employment connection and assess the case for contact”
  is CRM work.
- “Incorporate this report into what we know” is Annals work.
- “What does the corpus say about predicate locking?” is an Annals query.
- “Build the current public-facing narrative” is Weaver work.
- “Email me this update” is Email work.
- “What does this project mean by grounding?” is a Semantics query when that
  folder is registered.
- “Have we already solved something shaped like this?” is a Geste query.
- “What does Krisis promise an account consumer?” starts with Chancery
  discovery and exact-ID promise resolution.
- “Negotiate the exact CRM terms with each independently stewarded system
  before implementation” is a Pratica negotiation.
- “Could this new local project use an agent?” starts with the new-requester
  checklist in this manual.
- “Fix this now” is ordinary immediate work, not automatically a Todo.
- “Why did this Annals delivery fail?” requires Annals domain state and may also
  require the correlated Nucleus runtime record.

## Read supported and installed state separately

Do not maintain a dated table of installed versions in this manual. Read live
state from the installed programs and supported semantics from the versioned
source and contracts:

```sh
nucleus --version
nucleus health
nucleus service status
annals --version
annals-usage --version
todo --version
crm --version
email --version
conversations --version
krisis --version
semantics --version
geste --version
pratica --version
chancery --version
chancery doctor
weaver --version
```

`nucleus health` is strict. It prints the readiness document but exits nonzero
unless the daemon is compatible, authenticated, and accepting jobs. Its output
identifies the daemon and harness versions, executable, protocol versions,
adapter capabilities, authentication readiness, and the configured, active,
and available execution slots.

A requester's compiled Nucleus client source, the installed Nucleus patch
release, the public protocol version, the store schema, and the exact supported
Codex version are separate compatibility axes. A requester built from an older
compatible client revision can use a newer daemon when they share the same
public protocol. Do not require lockstep product releases without a contract
reason.

## Topology and authority

```text
Todo research -----\
CRM steward --------+
Annals -------------+
Weaver -------------+--> Nucleus ----------> isolated Codex app-server --> account
Krisis observer ----+
Semantics worker ---+
Pratica reviews ----/
   |                  |                        |
   |                  |                        `-- isolated job process
   |                  |
   |                  |-- Nucleus SQLite job authority and stdout atoms
   |                  |-- Nucleus-owned Codex authentication
   |                  `-- durable requester-tool mailbox
   |
   `-- requester-owned domain state, receipts, and recovery authority

Annals Usage <------ Nucleus output atoms and account reads
Todo SQLite <------- Todo's validated stage tools and explicit decisions
CRM SQLite <-------- CRM's queued intake and validated case revisions
Weaver outputs <---- Weaver's detached repository worker
Email -------------> Resend
Conversations ------> normal-user Codex app-server
Codex Stop hook ----> Krisis SQLite observation queue
Krisis observer ----> Conversations
Krisis observer ----> Annals decisions-library acceptance
Annals decision feed -> Semantics --> Conversations exact cwd
Semantics -----------> registered project semantic repositories
Pratica CLI ---------> Pratica SQLite offers, assent, seals, and reviews

interactive agent -- manual source anchors --> Geste SQLite episode revisions
Geste CLI ----------------------------------> Geste SQLite read projections

installed product releases -- publish --> Chancery provider bundles
interactive agent ----------- reads ----> Chancery
```

Todo's concern-routing, situation-assessment, and design-reconciliation
research follows the Todo-to-Nucleus arrow. Deterministic concern capture,
reads, lifecycle changes, routing/design authorization, and email commands use
Todo's SQLite database directly. In particular, an authorization command
cannot be invoked by a model, and the optional daily email path calls Resend
without creating a Nucleus job, using Nucleus authentication, or depending on
Nucleus health.

CRM is a short-lived local CLI, SQLite case library, and bounded Nucleus
requester.
`crm tell` stores its supplied content as SQLite `TEXT` and queues an
asynchronous steward run; CRM-owned content has no sidecar-file authority. Each
steward job uses requester program `crm`, immutable toolset
`crm/case-steward/1`, model `gpt-5.6-terra` at medium reasoning, a neutral
absolute temporary-directory working directory, workspace access `none`, no
local execution, no web search, no launch context, and a 1,200-second timeout.
Its only managed tool is `submit_case_revision`. A committed revision in the
CRM database, not Nucleus completion or model prose, is steward success. CRM is
the sole authority for that result. Advisory review notes are prominent on
every CRM consumption surface but never block capture, revision, or later
action. CRM has no scheduler and no direct-Codex fallback.

Weaver submits five content-only jobs in order. Its detached interactive-lineage
worker owns repository reads and atomic Markdown output writes; Nucleus and the
Codex process never receive repository filesystem authority. Weaver success
requires its persisted outputs and mechanical validation, not merely completed
Nucleus jobs.

Email is a synchronous CLI that sends plain-text messages directly to Resend.
It creates no Nucleus job, uses no Nucleus authentication, owns no daemon or
domain database, and does not depend on Nucleus health.

Conversations is a stateless read-only adapter over the normal user's Codex App
Server. It is deliberately separate from Nucleus's isolated per-job Codex home
and does not infer process liveness from persisted task status. Each invocation
owns and terminates a private process group for its selected Codex command and
inherited App Server descendants; unrelated Codex processes remain outside
that cleanup scope. Krisis' synchronous Codex `Stop` hook persists only a
session/turn correlation. Its single worker resolves each eligible completed
root turn through Conversations and uses Nucleus only for bounded account
classification. Every nonempty user authority in the turn receives a final
binary decision or no-decision coverage result; a file change is neither an
eligibility gate nor decision authority. Initial classification contains the
current-turn authorities, the nearest preceding nonempty assistant message for
each, and the final nonempty assistant message in the turn. One context
expansion may add earlier normalized messages, but either scope is capped at 64
whole messages and 262,144 UTF-8 text bytes and never truncates a message. An
oversized mandatory slice, incomplete source, ambiguous anchor, or failed read
is deferred or failed closed rather than recorded as no decision.

For each identified decision, Krisis records enough operational state to retry
one deterministic Markdown account containing context, decision statement,
observed action, observed result, and one exact user-authority source span.
Unobserved context, action, or result is represented explicitly; it is not
inferred. Krisis retains observation coverage, classifier correlation, its
outbox, and Annals acceptance receipts, but the dedicated Annals decisions
library is the durable account collection. Once the receipt commits, Krisis
retires its local account prose, quotation, and non-authority support anchors;
only the delivery ledger, digest, binary coverage, job correlation, and exact
authority anchor remain. Idempotent Annals acceptance is the delivery boundary.
Krisis does not retain an active candidate-review lifecycle,
send a digest, or judge confidence, disposition, truth, relevance, importance,
applicability, supersession, enactment, or current force. Its one-shot runner is
activated by the independent `krisis/observer` Clockwork binding only after a
separately authorized cutover; legacy Decisions schedules and lifecycle history
remain disabled, readable compatibility state.

Semantics is a project registry and one serial decision-reconciliation worker.
Existing projects start the new intake at one explicitly captured watermark in
the dedicated Annals decisions-library feed; new projects capture the current
feed watermark at registration. An exact `Semantics-Project: ID` line in the
registered root's `AGENTS.md` still establishes participation. The worker
resolves each accepted account's authority task through Conversations and routes
its exact cwd to the deepest registered root. A known cwd outside every root is
irrelevant; missing or non-unique authority and routing data remains explicit
unassigned intake. Every valid accepted account is eligible without confidence
or review gates. Semantics submits only its minimized account projection and the
selected repository snapshot to Nucleus, with workspace access `none`, no shell,
and no web. Its SQLite commit, not Annals retention, Nucleus completion, or
generated prose, is semantic authority. Legacy Decisions cursors, intake,
groundings, and correlations remain decodable history in a distinct namespace.
Chancery documents how agents discover and query the repository but is not a
runtime dependency.

Nucleus is a per-user execution coordinator, not a project registry or workflow
engine, capability directory, or documentation service. A requester submits one
closed, versioned invocation. Nucleus validates it, starts one harness attempt,
retains exact harness stdout, and coordinates dynamic tool calls. The requester
continues to own the work that motivated the job.

Geste is a short-lived manual casebook CLI. The interactive agent independently
consults source products through their installed contracts and gives Geste
stable locators, optional revisions or digests, and an authored complete
episode account. The Geste process calls no source product, Chancery, Nucleus,
or model. A historically verified settlement may still cite a retained legacy
Decisions lifecycle authority anchor; Geste has no automatic dependency on the
new Annals decision feed. Geste search returns
lexical precedent candidates; the agent checks applicability and current
contracts before reuse.

Pratica is a short-lived integration-negotiation CLI and Nucleus requester. It
stores exact opaque Markdown offers, fixed bilateral parties, current assent,
immutable agreement seals, source bases, and independent composition and
conformance reviews. Its three requester profiles have workspace access none,
no shell, no web, no launch context, and only closed frozen-source tools plus
one typed submission tool. Pratica domain commit—not Nucleus completion or
agent prose—is negotiation success. A seal proves exact assent on one basis;
it does not implement a target system, prove runtime behavior, discover every
concern, or authorize deployment.

Nucleus, Annals, Annals Usage, Todo, Chancery, Weaver, Email, Conversations,
Krisis, Semantics, Geste, Pratica, Clockwork, and CRM share the Cell source
repository, Cargo workspace, and lockfile. That source layout does not
merge their release, installation, state, backup, recovery, or domain-success
boundaries. Product runtimes do not call Chancery. Their installers only
co-stage owned documentation and publish their owned provider selectors.

The following distinctions are operationally important:

1. **Nucleus completion is not domain success.** Todo succeeds when its
   stage-specific proposal, assessment, or design operation is durably
   recorded; authorization is a separate Todo decision. Annals succeeds
   according to its retained reconciliation and delivery state. Weaver succeeds
   only after it persists and validates the required narrative outputs.
   Semantics succeeds when its validated revision and intake receipt are
   durable. CRM steward work succeeds when its validated case revision is
   committed. A model's final prose is diagnostic.
2. **A domain commit can outlive a runtime failure.** If Todo records its
   validated stage result, Annals records its reconciliation, or Semantics
   appends its revision, or CRM commits its case revision and Codex later fails,
   the durable domain result remains authoritative.
3. **A requester restart differs from a daemon restart.** A requester can
   rediscover a durable pending tool call. A Nucleus restart cannot resume a
   Codex process; startup marks unfinished attempts `lost` and their jobs failed.
4. **Registrations are immutable.** Schema IDs and toolset identities preserve
   historical decoding. Change their versions instead of editing old records.
5. **Reporting is calculated.** Annals Usage joins requester attribution to
   Nucleus output atoms without retaining a second reporting database.
6. **One Nucleus job has one attempt.** Nucleus does not automatically retry.
   A requester owns any new domain attempt and its provenance.
7. **A Geste precedent is not policy.** An episode preserves one authored
   account against one source cutoff. Similar shape does not establish current
   applicability, authority, or procedure.
8. **A Pratica agreement is not implementation proof.** It preserves exact
   party assent and basis. Composition is advisory, and conformance reviews one
   supplied candidate snapshot without testing, changing, or deploying it.
9. **A CRM advisory is not a gate.** It remains conspicuous wherever the case
   is consumed, but it cannot block or authorize any operation.

### Shared CI, release, and deployment coordination

Every public product `ci.sh` is a synchronous client of one current-user Cell
CI broker. The broker keys its production scope by this host and the Git common
directory, so linked worktrees share admission rather than creating one
compiler budget and target directory each. Python 3.10 or newer is a CI
bootstrap prerequisite.

The heavy lane admits exactly one Cargo gate at a time, sets
`CARGO_BUILD_JOBS=2` and `CARGO_INCREMENTAL=0`, and points every linked worktree
at the primary checkout's one `target` directory. A separate two-slot light
lane is only for bodies that do not invoke Cargo. Requests are FIFO. Only an
exact identical Git-clean source, gate, toolchain, allowed environment, command,
and relative working directory may join work already queued or running; dirty
candidates never join, and completed results are never reused. Queued and
running leases expire fail-closed, abandoned running work becomes `lost`, and
no crash or stale result becomes green. The journal is under
`~/Library/Application Support/Cell/ci-broker`; use
`python3 ci_broker/client.py status EXECUTION_ID` or `recover` from a Cell
checkout for diagnosis.

The root `./ci.sh` first records one exact source key, passes that expected key
to each independently scheduled product gate, and rejects the plan with exit
75 if the worktree changes. A complete run then rebuilds Chancery for that same
candidate and validates the integrated fifteen-provider, 52-entry source graph.
This aggregate evidence does not merge product release authority or turn one
product gate into another's gate.

Each product release entry point uses its checked-in descriptor but remains a
separate release unit. It holds one Git-common-directory publication lock
through its brokered product gate, commit, tag, and atomic push, and rechecks
the expected `origin/main` revision and absent tag before publication. A
release command still requires separate explicit authority; CI never invokes
one.

Deployment remains product-owned. Every deployer takes its existing product or
update lock before the shared Chancery catalog-writer lock and holds the catalog
lock through selector cutover, smoke, and rollback. Generated selector-only
deployers for Conversations, Geste, Pratica, and CRM stage immutable bytes
before the short catalog critical section, publish command and provider through
one atomic product `current` selector, and reject a changed observed or explicit
`--expected-current absent|releases/HASH` precondition. Stateful deployers keep
their product-specific quiescence, migration, service, and recovery logic and
are conservatively globally conflicting for orchestration. Catalog presence,
CI success, and release preparation grant no deployment authority.

### Standard installed paths

```text
~/.local/bin/nucleus
~/.local/libexec/nucleusd
~/.local/bin/email
~/.local/bin/conversations
~/.local/bin/krisis
~/.local/bin/semantics
~/.local/bin/geste
~/.local/bin/pratica
~/.local/bin/clockwork
~/.local/bin/crm
~/.codex/hooks.json
~/Library/LaunchAgents/org.nucleus.daemon.plist
~/Library/LaunchAgents/org.clockwork.annals.inbox.plist
~/Library/LaunchAgents/org.clockwork.annals.decisions-inbox.plist
~/Library/LaunchAgents/org.clockwork.krisis.observer.plist
~/Library/LaunchAgents/org.clockwork.semantics.worker.plist

~/Library/Application Support/Chancery/providers/
  PROVIDER_ID -> owning product's current release share/chancery/PROVIDER_ID

Use `chancery doctor` for the current complete provider set. One product
release may publish multiple providers, as Annals/Annals Usage and
Krisis/retained Decisions compatibility do.

~/Library/Application Support/Nucleus/
  install/
    releases/RELEASE_ID/
      bin/nucleus
      libexec/nucleusd
      share/chancery/nucleus/
    current -> releases/RELEASE_ID
    previous -> releases/RELEASE_ID
  nucleus.sock
  nucleus.db
  nucleus.db-wal
  nucleus.db-shm
  codex-home/

~/Library/Logs/Nucleus/
  nucleusd.stdout.log
  nucleusd.stderr.log

~/Library/Application Support/Annals/decisions/
  config.toml
  annals.db
  spool/
  log/inbox.stdout.log
  log/inbox.stderr.log

~/Library/Application Support/Decisions/
  decisions.db
  install/

~/Library/Logs/Decisions/
  observer.stdout.log
  observer.stderr.log

~/Library/Application Support/Semantics/
  semantics.db
  install/

~/Library/Application Support/Geste/
  geste.db
  install/

~/Library/Application Support/Pratica/
  pratica.db
  install/

~/Library/Application Support/CRM/
  crm.db
  install/

~/Library/Logs/Semantics/
  worker.stdout.log
  worker.stderr.log

~/Library/Application Support/Clockwork/
  clockwork.db
  install/
```

The four `org.clockwork.*` paths above are the successor state after a
separately authorized Clockwork and product deployment. This source change does
not install Clockwork or perform that cutover. Until then, the existing
Annals inbox scheduler plus `org.decisions.daily-email`,
`org.decisions.observer`, and `org.semantics.worker` LaunchAgents remain the
installed runtime truth. Never load a legacy product scheduler and its
Clockwork successor together.

The complete Nucleus state directory is sensitive. The database can contain
prompts, tool arguments and results, source content emitted by an agent, and
exact non-authentication app-server stdout. Host-managed authentication
responses and managed-worker stderr are excluded from job events and durable
output. The Nucleus-owned Codex home contains the
authoritative credential and may contain additional Codex-local state. The
Unix socket has no application-level authentication; local user ownership and
filesystem permissions are the trust boundary. There is no TCP listener in
protocol version 1.

Email's user-owned installer owns `~/.local/bin/email`, its content-addressed
releases under `~/Library/Application Support/Email/install/`, and the
`providers/email` selector. Email keeps no application state; send readiness
depends on the installed binary, `RESEND_API_KEY`, and Resend, not on Nucleus or
Chancery readiness.

Annals, Todo, and Weaver have their own state, installation, backup, and recovery
boundaries. Do not infer their state from Nucleus or copy their detailed
procedures into this manual. The primary Annals library and dedicated decisions
library are separate databases, spools, configs, logs, histories, and recovery
units under the same product. Clockwork owns independent 300-second,
run-at-load `annals/inbox` and `annals/decisions-inbox` activation bindings and
their runtime histories, while Annals owns each selected library's durable
spool, locks, retries, maintenance/pause gates, logs, and domain outcomes. The
primary Annals deployer does not create the decisions library. Annals'
supported decisions provisioner must be invoked from an exact format-4 content
release; that release independently hashes the provisioner and both decisions
templates. It creates or migrates only the dedicated config, database, spool,
logs, and `annals/decisions-inbox` binding. Use its `--keep-maintenance`
handoff while the outer Krisis/Semantics cutover establishes the feed watermark
and consumer activation. The pre-cutover primary Annals scheduler remains
installed truth until a separately authorized handoff. Pause each active Annals
config independently when its domain admission must stop. Todo's optional
`~/Library/LaunchAgents/org.todo.daily-email.plist` is a separate user service:
launchd invokes Todo at 09:00 machine-local time, its zsh runner sources
`RESEND_API_KEY` from `~/.zshrc`, and its logs live under
`~/Library/Logs/Todo/`. It is not part of `org.nucleus.daemon` or Nucleus's
authentication authority.

Conversations has a content-addressed installation but no application database.
Krisis owns its additive schema-version-4 database, write-once activation
baseline, observation coverage, Nucleus correlations, account outbox, Annals
receipts, installed releases, provider selector, exact user `Stop` hook,
release-local observer runner, and body-free logs. The database and logs retain
their historical `Decisions` filesystem locations so the rename does not split
persistent history. Clockwork owns the successor 60-second `krisis/observer`
binding and process history; a pre-cutover Decisions release still owns the two
legacy `decisions/observer` and `decisions/daily-email` schedule projections,
whether Clockwork bindings or older LaunchAgents, which are disabled rather
than renamed during the separately authorized cutover. The
deployer refuses any pre-existing foreign `~/.codex/hooks.json`; it never merges,
overwrites, removes, or trusts one. Codex owns exact-definition review through
`/hooks`, and the actual client surface must be canaried after trust. The Krisis
observer is a Nucleus requester, so quiesce it for Nucleus maintenance. A
Krisis schema cutover additionally suspends its public hook command and drains
the three-second hook timeout before the SQLite backup.
Default write-once activation stores the next whole Unix second, excluding the
cutover second; only after that durable boundary does deployment publish the
live hook, command, bindings, and services. Missed events are reconciled
afterward. If rollback cannot prove database quiescence or restore every
artifact, its release-independent maintenance gate remains, scheduler cleanup
is attempted, and the public command is removed when that can be proved while
the private transaction backup is retained. Legacy Decisions review and digest
state stays readable history; Krisis does not call Email.

Semantics owns its content-addressed installation, provider selector,
schema-version-2 database, body-free worker logs, and release-local worker
runner. Clockwork owns the successor 60-second `semantics/worker` schedule
binding and process history; a pre-cutover Semantics release still owns its
legacy LaunchAgent. The worker is the only automatic reconciler and admits at most one
Nucleus job at a time. Deployment stops that worker, proves database
quiescence, preserves the database and sidecars for rollback, validates the
candidate against the exact Annals decisions library, Conversations, and
Nucleus, and refuses the worker switch whenever an active or paused project
lacks the selected feed identity or both Annals cursors. A pending feed
activation is ready only when no such project exists. Deployment then
publishes its selectors and schedule binding. Project folders contain only the participation
marker; they do not contain or own Semantics database state.

Clockwork is a separate non-agent scheduling product, not a Nucleus job mode.
It records immutable launch definitions, binding state, and direct process
outcomes only. Annals, Krisis, and Semantics retain their domain queues, locks,
retry/idempotency rules, secrets, logs, and meanings of success; Nucleus remains
the agent-execution dependency those products may call. Clockwork program
deployment requires a separately supplied candidate Chancery reader to
validate its exact staged provider before either public selector changes.

Geste owns its content-addressed CLI installation, provider selector, and
schema-version-1 database. Deployment switches only immutable program and
provider selectors; `geste init` is a separate visible domain-state operation.
There is no daemon, LaunchAgent, automatic migration, source adapter, model
request, or runtime Chancery dependency. Each episode revision is sealed last
in its capture transaction; doctor requires the complete schema object set and
refuses committed unsealed history. The database is retained separately from
releases and has no automatic pruning or version-0.1 uninstaller. After a
post-commit domain-canary failure, redeploy the exact binary with the packaged
deployer selected by `install/previous`; do not rewrite selectors or episode
state by hand.

Pratica owns its content-addressed CLI installation, provider selector, and
schema-version-2 negotiation database, including caller-keyed ingress receipts.
Deployment switches only immutable program and provider selectors and never
opens or migrates the database; `pratica init` is separate. The only supported
schema upgrade is an explicit, quiescent version-1-to-version-2 migration that
refuses active attempts and first writes a caller-selected private SQLite
backup. Rollback after that migration requires the retained version-1 backup
with the old binary; switching program/provider selectors alone is insufficient.
There is no daemon, LaunchAgent, schedule, runtime Chancery dependency, source
crawler, direct Codex path, automatic retry, or version-0.1 uninstaller.
Explicit steward, composition, and conformance commands synchronously use
Nucleus; stdin transport and caller-file ownership remain entirely Pratica and
caller concerns.

CRM owns its content-addressed CLI installation, provider selector, and local
SQLite library. All CRM-owned content is stored as database `TEXT`. Deployment
switches immutable program and provider selectors without adding a daemon or
schedule. `crm tell` queues an asynchronous steward run, which uses Nucleus and
never falls back to a direct Codex invocation.

## Compatibility model

| Axis | Authority | How to inspect | Change consequence |
| --- | --- | --- | --- |
| Nucleus release | CLI and daemon package versions | `nucleus --version`, `nucleus health` | Candidate CLI and daemon versions must match. A patch need not force requester releases when public semantics are unchanged. |
| Capability contract | Product Chancery bundle | `chancery show nucleus.execution.operate` | Incompatible operational meaning, including the move from serialized execution to eight slots, increments the entry contract even when the HTTP protocol remains wire-compatible. Audit and widen each compatible requester's dependency bound before deployment. |
| Public invocation protocol | `nucleus-core`, HTTP contract, runtime contract | `supportedProtocolVersions` in health and the request types | Additive support can be deployed daemon-first. An incompatible change requires a new protocol version and coordinated requester cutover. |
| Nucleus store schema | `nucleus-store` schema and migration code | SQLite `PRAGMA user_version` and the source constant | A newer schema can make binary-only rollback unsafe. It needs an explicit migration and database rollback plan. |
| Codex harness | `nucleus-codex` adapter and semantic checks | Harness identity in health | The adapter supports an exact inspected Codex release. Update and prove the adapter before replacing the executable. |
| Requester client build | Shared Cargo workspace and requester adapter | Workspace manifests, lockfile, and requester source revision | Rebuild when it needs changed types or behavior. Runtime compatibility still follows the public protocol, not source lockstep. |
| Output decoder and toolset | Attempt harness identity, immutable Nucleus registrations, and requester code | Harness version plus registration identity and digest | Keep a decoder for each retained harness version; publish a new schema ID or toolset version when requester-owned meanings change. |
| Requester domain schema | Requester's database and migrations | Requester-specific validation and doctor commands | The requester owns migration, backup, success, and rollback. Nucleus must not duplicate it. |

When a portable job meaning changes, update the core types, client, runtime
contract, examples, HTTP contract tests, and every affected requester. When a
change is backward compatible, deploy support to Nucleus before a requester
emits the new form. When the old and new forms cannot coexist, prevent new
requester work, let active jobs settle, take the required backups, and perform
a coordinated cutover.

## Routine operation

### Read readiness and current work

Start diagnosis with supported interfaces, not process-tree inspection:

```sh
nucleus health
nucleus service status
nucleus account --wait 0
nucleus jobs list --state accepted
nucleus jobs list --state running
nucleus jobs list --state waiting-on-requester
nucleus jobs list --state failed
```

Inspect one job and its ordered harness-output records with:

```sh
nucleus jobs show JOB_ID
nucleus jobs logs JOB_ID
nucleus jobs logs --follow JOB_ID
```

Scope a search to one domain run when the requester identity is known:

```sh
nucleus jobs list --requester PROGRAM --requester-id REQUESTER_ID
```

Health's `execution` object reports `maxActiveJobs=8`, the number of slots held
by live attempts, and the slots immediately available. A job beyond that limit
remains `accepted` with a `pending` attempt. Its invocation timeout starts when
it acquires a slot, and a `waiting_on_requester` attempt keeps its slot because
its app-server remains live. `acceptingJobs` is admission readiness, not a claim
that a slot is currently free.

`nucleus account --wait 0` is a nonblocking canonical-credential probe. An
`authentication_busy` result means another account, refresh, or login operation
currently owns that short exclusive boundary; running jobs alone do not make
the account probe busy, and contention does not by itself mean the credential
is invalid.

### Quiesce before work that cannot tolerate a lost attempt

Nucleus has no global drain mode. Quiescence is established at its requesters:

1. Do not start a synchronous Todo creation, invoke `crm tell`, start a new
   Weaver submission, run a Pratica steward/composition/conformance review,
   invoke `krisis observe process`, or start another manual requester job.
2. If Weaver has a nonterminal current run, select its exact run ID and let it
   settle through `weaver wait RUN_ID`.
3. Pause both Annals library inboxes that are active and wait for their
   independent deliveries to settle:

   ```sh
   annals inbox pause
   annals inbox status
   annals --config "$HOME/Library/Application Support/Annals/decisions/config.toml" inbox pause
   annals --config "$HOME/Library/Application Support/Annals/decisions/config.toml" inbox status
   ```

4. Stop the Krisis observer and the Semantics worker so periodic work
   cannot admit new jobs. On a Clockwork-cut-over installation, first capture
   each selected digest **and enabled state** with `binding show`, then disable
   only keys that were enabled. Leave an already disabled or absent key
   unchanged. On a legacy installation, boot out the three old product labels instead.
   Never operate both scheduler forms for one runner. The `Stop` hook may still
   enqueue a content-free correlation, which is safe to process after
   maintenance:

   ```sh
   clockwork binding show krisis/observer
   clockwork binding show semantics/worker
   # Repeat only for each key whose show result says enabled=true:
   clockwork binding disable OWNER/NAME
   ```

   Legacy alternative:

   ```sh
   launchctl bootout "gui/$(id -u)/org.decisions.observer"
   launchctl bootout "gui/$(id -u)/org.decisions.daily-email"
   launchctl bootout "gui/$(id -u)/org.semantics.worker"
   ```

5. Inspect Nucleus `accepted`, `running`, and `waiting-on-requester` jobs.
6. Wait for them to become terminal. Cancel a job only when abandoning that
   exact runtime attempt is intended:

   ```sh
   nucleus jobs cancel JOB_ID
   ```

7. Perform the service, storage, or harness operation.
8. Verify Nucleus and requester canaries before resuming the Annals inboxes,
   Krisis observer, Semantics worker, or new Weaver or CRM work. For Clockwork,
   switch only a key that step 4 recorded as enabled back to its exact captured
   digest; leave every originally disabled or absent key unchanged. For a
   legacy install, bootstrap only the exact previously loaded owned product
   plists.

Graceful shutdown first requests cancellation. On startup, every attempt still
durably nonterminal is marked `lost`. The requester—not Nucleus—decides whether
a new domain attempt is safe.
The Todo daily-email LaunchAgent is not a Nucleus requester and does not need
to be paused to establish Nucleus quiescence. Krisis' observer can start a
classification job every 60 seconds, and Semantics can start one reconciliation
job every 60 seconds. Restore only the bindings or legacy services captured as
enabled after Nucleus is ready. Do not reset the Krisis baseline, observation
coverage or outbox, or either Semantics cursor namespace: their durable state is
the intended recovery path.

### Authentication recovery

Nucleus owns one authoritative Codex home under its private state directory.
Managed ChatGPT jobs receive in-memory access-token and account metadata;
they never receive the refresh token or write authentication back. Concurrent
401 requests are coalesced against the credential generation, and the one
canonical refresh path advances `auth.json` under an exclusive mutation lease
through a validated, fsynced atomic promotion from a private staging home.
Once elected, that broker operation survives cancellation of its requesting
job. Graceful daemon shutdown closes new broker work, repeats job cancellation
after HTTP handlers drain, and waits for existing broker activity to settle.
Static API-key jobs use isolated credential snapshots without copy-back.
Account reads use the same short canonical-operation boundary and can overlap
running jobs. They run in private staging because Codex may proactively refresh;
request cancellation or timeout cannot interrupt canonical persistence, and a
valid same-account generation is atomically promoted before the broker settles.
Attended login takes the exclusive authentication-session barrier, waits for
every active job session to settle, writes only private staging, and promotes a
validated credential only after successful completion. Annals, Todo, and CRM do
not read or refresh the credential themselves.

For attended recovery:

1. Prevent new requester work and let active work settle.
2. Run the Nucleus-owned login, directly or through Annals Usage:

   ```sh
   nucleus auth login --device-auth
   nucleus account --wait 0
   nucleus health
   ```

   ```sh
   annals-usage login --device-auth
   annals-usage doctor
   ```

3. Run a deliberate requester canary.
4. Resume paused dispatch only after the canary is understood.

Credential state is forward-only. Installation rollback deliberately restores
binaries and service configuration without restoring an older `auth.json`.
Do not overwrite a credential that may have been refreshed by a later daemon.

### Backup Nucleus state

Nucleus has no automatic output-retention, pruning, backup, or restore command.
Use an explicit, private backup destination and preserve its access controls.

For a backup intended to support service recovery:

1. Quiesce requesters and wait for Nucleus jobs to become terminal.
2. Record `nucleus --version`, `nucleus health`, and the selected Codex
   executable. These establish the reader and harness needed by the backup.
3. Boot out the user service so no database or credential writer remains:

   ```sh
   launchctl bootout "gui/$(id -u)/org.nucleus.daemon"
   ```

4. Create a SQLite-aware backup of
   `~/Library/Application Support/Nucleus/nucleus.db`. With the daemon stopped,
   a SQLite backup tool may safely open the closed database. If another method
   copies SQLite files, it must preserve the database and any WAL sidecars as
   one consistent set; copying only the main database while it is live is not a
   backup.
5. Separately back up the private Nucleus-owned Codex home when credential
   recovery is in scope. Treat that copy as authentication material, not as an
   ordinary document archive.
6. Include the LaunchAgent, daemon logs, and requester state only when the
   recovery objective requires them. Nucleus state does not replace Annals,
   Todo, or CRM backups.
7. Bootstrap the unchanged service and verify health:

   ```sh
   launchctl bootstrap "gui/$(id -u)" \
     "$HOME/Library/LaunchAgents/org.nucleus.daemon.plist"
   nucleus health
   ```

Restoration is an attended operation. Quiesce and stop the service, preserve
the current state separately, restore a database only to a binary known to
support its schema, then validate health and retained job/output reads before
requester canaries. Do not restore old credentials as part of an ordinary
database rollback. If authentication itself must be recovered, make that an
explicit choice and expect attended login to be safer than replacing a newer
credential with an older copy.

### Retention and storage

Monitor both Nucleus state and logs:

```sh
du -sh "$HOME/Library/Application Support/Nucleus"
du -sh "$HOME/Library/Logs/Nucleus"
```

Apply the host's normal private-log rotation policy to the LaunchAgent's stdout
and stderr files. Do not delete rows from `nucleus.db`, edit immutable
registrations, or remove individual Codex-home files as ad hoc retention. The
database's reporting ledger already excludes harness input, lifecycle/control
events, host-managed authentication responses, stderr chunks, requester
results, and calculated aggregates; it keeps one exact row per remaining
harness stdout record. A supported pruning policy must first
define which observations and coordination relationships remain valid,
implement that policy in Nucleus, and include migration and recovery tests.
Until then, monitor, back up, and retain the state.

### Uninstall and restart semantics

`nucleus service restart` terminates the current daemon and asks launchd to
start it again. Quiesce first when active-attempt continuity matters.

`nucleus service uninstall` removes the LaunchAgent and installed Nucleus
binaries but deliberately retains Nucleus state and logs. Removing retained
state is a separate, explicit, destructive decision after its recovery and
retention obligations have been satisfied.

## Add a new requester

There is no `nucleus project create`. Connecting a project means implementing
an application integration whose domain boundary remains outside Nucleus.

### 1. Define the domain result before the runtime request

Write down:

- the durable condition that means the domain operation succeeded;
- which database, filesystem, or service is authoritative for that condition;
- which dynamic tools may mutate that authority;
- how duplicate delivery is made idempotent;
- who decides whether another attempt is safe after failure; and
- what a human can inspect to distinguish domain success from runtime success.

If those answers require Nucleus to understand the domain record, the boundary
is wrong. Nucleus should know requester identity, invocation, and tool protocol,
not Todo fields, Annals revisions, or another application's workflow states.

### 2. Select the client and compatibility boundary

Rust requesters in Cell should use its workspace `nucleus-core` and
`nucleus-client` sources without crate version pins. Another language may
implement the documented HTTP API over the per-user Unix socket. Do not shell
out to the human CLI as the application protocol when the typed client or HTTP
surface is available.

At startup or before work, require strict health and verify the protocol and
capabilities the requester needs. Do not compare only the daemon patch string.
Select the standard socket unless the application deliberately supports
`NUCLEUS_SOCKET` or another explicit override.

### 3. Define identity and provenance

- Choose a stable, lowercase requester `program` slug.
- Give each domain run a stable `requester.id` so all its Nucleus jobs can be
  queried together.
- Choose a unique Nucleus job ID. It is the idempotency key.
- On an ambiguous submission failure, retry only the byte-equivalent typed
  request with the same job ID. Reusing the ID for different content is a
  conflict.
- Use `parent` only for invocation provenance. It does not create workflow
  execution or retry semantics.
- Persist enough correlation in the requester to locate the Nucleus job and
  enough requester identity in the Nucleus request to locate the domain run.

### 4. Choose the complete invocation policy

Specify every supported semantic deliberately:

- exact harness and model;
- optional reasoning effort;
- an absolute working directory;
- workspace access: `none`, `read-only`, or `read-write`;
- explicit local-execution and web-search flags;
- a positive timeout;
- an optional immutable toolset reference; and
- an optional short-lived launch context.

Keep base `instructions`, optional `developerInstructions`, and the per-job
`prompt` in their distinct roles. Nucleus forwards them separately.

All version-1 jobs are ephemeral and unattended, use approvals disabled, and
have one attempt. Nucleus accepts no command, arbitrary argv, retry count,
workflow graph, or requester-defined Codex configuration.

For concurrent `read-write` jobs, give each job a disjoint working directory or
worktree, or serialize access in the requester. Nucleus limits process capacity;
it does not detect shared working directories or arbitrate conflicting
filesystem or external mutations.

Use a launch context only when the job must observe the requester's caller
environment. Register the complete snapshot immediately before submission. The
context is requester-bound, memory-only, single-use, expires after 120 seconds,
and is not a durable configuration mechanism.

Minimize authority. Todo's current concern-routing, situation-assessment, and
design-reconciliation jobs use `workspaceAccess=none`,
`builtinTools.localExecution=false`, `builtinTools.webSearch=false`, no launch
context, and only their bounded managed tools. The immutable
historical `create_todo` contract used a broader read-only research profile;
do not copy that legacy profile into current jobs. Annals likewise receives
neither builtin shell nor web access and operates through bounded tools. A new
requester should justify its own combination instead of copying either profile.

### 5. Define immutable schemas and tools

Namespace schema IDs and toolset identities to the requester. The toolset
provider must match the requester program. Register the exact definitions
before submitting a job that references them.

Registrations are immutable by identity and content digest. When arguments,
results, tool meaning, or definitions change incompatibly:

1. publish a new schema ID or toolset version;
2. keep the old decoder for retained historical jobs;
3. deploy code that understands the new version; and
4. submit new jobs referencing it.

Never update old registration rows in SQLite.

Todo's current immutable requester toolsets are
`todo/concern-routing/1`, `todo/situation-assessment/1`, and
`todo/design-reconciliation/1`. Their validated calls may write Todo-owned
`rN`, `aN`, or `dN` state, but cannot authorize routing, accept a design, or
execute implementation. The historical Todo `create_todo` schema and toolset
remain immutable for compatibility; current `todo new` does not use them.

CRM's immutable requester toolset is `crm/case-steward/1`. It exposes only
`submit_case_revision`; the model receives no managed read, search, or effect
tool and no builtin filesystem or web access. CRM validates that one typed
submission and atomically commits the revision in its own SQLite database. The
tool cannot send contact, schedule work, or turn an advisory note into an
authorization or blocker.

Krisis' current immutable requester toolset is
`krisis/decision-account-classification/1`. Its sole managed tool,
`submit_decision_account_classification`, returns complete per-authority
decision or no-decision verdicts and the normalized account projections for one
serial observation scope. Context expansion is an operational nonterminal
result, not a third verdict. The historical `decisions/turn-classification/1`
and `decisions/daily-classification/1` registrations and decoders remain
immutable only for retained legacy recovery; Krisis creates no new review or
daily-classification work.

Semantics uses the successor immutable
`semantics/semantic-account-reconciliation/1` toolset for new Annals accounts.
Its one managed call, `commit_account_semantic_reconciliation`, validates and
atomically appends a project semantic revision with exact library, event, and
account grounding. Historical Decisions correlations continue to use the
immutable `semantics/semantic-reconciliation/1` decoder. Neither toolset can
read the project filesystem, use shell or web tools, or mutate Annals or Krisis
state.

Pratica's immutable requester toolsets are `pratica/steward-response/1`,
`pratica/composition-review/1`, and `pratica/conformance-review/1`. Each exposes
`source_catalog`, `source_read`, and `source_search` over an exact closed UTF-8
snapshot, plus respectively `submit_steward_response`,
`submit_composition_review`, or `submit_conformance_review`. Their jobs use
workspace none, a deterministic neutral absolute cwd, local execution and web
disabled, no launch context, and one Nucleus attempt. The source catalog rejects
symlinks, sensitive or binary/control content, files over 4 MiB, and aggregate
content over 32 MiB. Pratica validates and commits the typed response; none of
these tools can implement a target system or make model prose authoritative.

### 6. Implement the lifecycle

A normal requester flow is:

1. Check Nucleus readiness and, when domain admission depends on it, account
   readiness.
2. Register required schemas and toolsets idempotently.
3. Register a launch context if needed.
4. Submit the exact request. When all eight slots are occupied, tolerate the
   job remaining `accepted` with a `pending` attempt until a slot opens; its
   invocation timeout begins only after slot acquisition.
5. Long-poll the durable tool-call mailbox while the job is nonterminal.
6. Validate each call, perform the domain operation through the requester's
   backend, durably bind or cache its exact result, and post that result.
7. Read the terminal job and structured attempt output.
8. Use the requester's durable state to decide domain success.
9. Read the output-only Nucleus ledger for protocol diagnostics or reporting,
   not as a replacement for the domain result.

The requester must recover after an ambiguous transport failure without
executing a domain mutation twice. The exact implementation belongs with its
database transaction and idempotency rules.

Fail clearly when Nucleus is unavailable or incompatible. Do not retain a
hidden direct-Codex fallback; two execution paths create two authentication,
recovery, and observability stories.

### 7. Define failure and retry behavior

Account for these cases explicitly:

- failure before admission;
- identical or conflicting resubmission;
- failure before a tool call;
- requester exit while a tool call is pending;
- ambiguous tool-result transport;
- domain commit followed by harness failure;
- saturation and accepted/pending queueing;
- queued cancellation before a start timestamp;
- active cancellation, or timeout after slot acquisition;
- a requester-tool wait retaining one execution slot;
- overlapping read-write jobs aimed at the same workspace or mutation target;
- daemon restart and a `lost` attempt;
- Nucleus completion without the required domain result; and
- domain success despite later runtime failure.

A genuinely new attempt uses a new job ID. Preserve the same domain-run
correlation when appropriate. Nucleus must not decide whether that attempt is
allowed.

### 8. Add observability, security, and acceptance proofs

At minimum, test:

- strict health and required capabilities;
- eight simultaneous active attempts with later work accepted/pending;
- successful admission and domain completion;
- identical and conflicting duplicate job submissions;
- identical and conflicting duplicate tool results;
- requester restart while waiting on a durable tool call;
- daemon loss during an active attempt;
- queued and active cancellation, timeout beginning after slot acquisition,
  and waiting-on-requester slot retention;
- disjoint worktree or requester-lock enforcement for concurrent write-capable
  jobs;
- authentication busy and unavailable behavior;
- unsupported model, harness, working directory, or permission combinations;
- durable domain success followed by runtime failure; and
- absence of a direct-runner fallback.

Treat both the requester's state and Nucleus output atoms as sensitive according to
the content they can retain. Add a requester canary, backup coverage, release
ordering, rollback boundary, and operator documentation before production use.

### 9. Add its capability relationship

If the requester exposes a distinct user-facing durable outcome or supported
local-product consumer surface, publish a product-owned Chancery entry and
detailed manual with its release. Give its provider a promise scope that names
the product's jurisdiction and a meaningful complete or partial inventory
boundary. Give the entry a plain-language title and outcome-discriminative
summary, then state when it does and does not apply, its effects, authority,
success, recovery, privacy, interfaces, and dependencies.

Normalize the consumers, preconditions, inputs, outputs, data semantics,
identity and units, completeness and freshness, access, lifecycle and
consistency, limits, compatibility and evolution, and substantive reliances.
Mark each claim declared, unsupported, unspecified, or not applicable. Keep
documentation-contract dependencies distinct from runtime, data, authority,
readiness, and external reliance; do not mechanically reinterpret old edges.

Make the product installer own each Chancery provider selector it publishes. Validate
the source bundle in product CI and prove installed list/show discovery and
exact-ID resolution during deployment. A resolver gap is an owning-product
contract gap, not permission to infer a promise from code or schema.

Do not copy the card into this manual or global discovery instructions. Global
instructions contain only the Chancery bootstrap; exact behavior stays in the
version-matched product bundle. Nucleus remains runtime authority and gains no
provider registry or documentation storage.

## Route changes by their authority

| Change | Primary authority | Cross-system obligations |
| --- | --- | --- |
| Todo concerns, routing and explicit decisions, identities, assessments, designs, lifecycle, provenance, database, email delivery, or deployment | Todo | Preserve its Nucleus adapter contract when affected; the direct Resend path does not become a Nucleus job, and Nucleus does not gain Todo fields. |
| CRM intake, cases, evidence, revisions, advisories, queued steward runs, database, or deployment | CRM | Preserve its bounded Nucleus adapter and prominent nonblocking advisories; Nucleus gains no CRM fields, domain success, scheduling, or retry authority. |
| Annals works, physical-library identity, concepts, evidence, reconciliation, inbox, producer acceptance, decision feed, retry, or corpus migration | Annals | Keep primary and decisions libraries isolated; preserve job correlation and adapter behavior when affected; Nucleus does not gain Annals workflow state. |
| Annals usage attribution, budget display, or diagnostic projection | Annals Usage | Read Nucleus records through the supported interfaces; do not become runtime or corpus authority. |
| Weaver workflow state, stage prompts, repository inputs or outputs, validation, cancellation, recovery, or deployment | Weaver | Preserve its Nucleus invocation and correlation contract; Nucleus does not gain narrative repository authority or retry policy. |
| Email content, delivery, Resend access, fixed addresses, or deployment | Email | Keep the direct Resend path independent of Nucleus; Nucleus gains no email fields, credential, or delivery authority. |
| Codex task enumeration, normalized transcript reads, App Server compatibility, or Conversations deployment | Conversations | Keep it read-only and separate from Nucleus's private Codex home; consumers must not treat persisted status as live-process proof. |
| Decision identification, observation coverage, account projection, source anchors, Annals delivery, or Krisis deployment | Krisis | Preserve exact user authority, deterministic account identity, Annals acceptance receipts, and Nucleus correlation; no downstream consumer gains classification authority and Nucleus gains no decision fields. |
| Project registration, semantic concepts, grounding, revision history, Annals decision-account intake, reconciliation policy, or Semantics deployment | Semantics | Preserve Annals library/event/account identities, exact Conversations cwd routing, both legacy and new cursor histories, and Nucleus correlation; no upstream gains Semantics state or success authority. |
| Geste episode identity, revisions, settlement grounding, search, report, graph, database, or deployment | Geste | Preserve source-system authority and immutable locators; no source gains episode state, and Geste gains no automatic source read or policy authority. |
| Pratica steward scopes, offers, assent, agreement seals, bases, attempts, composition, conformance, database, or deployment | Pratica | Preserve exact opaque Markdown, target-system authority, frozen source disclosure, and Nucleus correlation; neither Nucleus nor a review gains party, implementation, or retry authority. |
| New portable invocation meaning or HTTP behavior | Nucleus core/client/daemon | Version the public contract, update examples/tests/docs, then update affected requesters in compatible order. |
| Codex executable or app-server semantics | Nucleus Codex adapter | Prove the exact version, deploy Nucleus, then run generic and requester canaries. |
| Nucleus database schema or retention | Nucleus store | Quiesce, back up, migrate and validate, and define database-aware rollback before deployment. |
| Requester tool arguments, result, or definition | Requester plus immutable Nucleus registration | Publish a new schema/toolset version and keep historical decoding. |
| Requester prompt, model, timeout, or permission profile | Requester | Use new job IDs for new attempts, verify health capabilities, and rerun domain acceptance tests. |
| Managed-authentication, canonical-refresh, or attended-login behavior | Nucleus | Quiesce all credential consumers, preserve forward-only authentication, and canary every requester. |
| Nucleus service layout or installer | Nucleus CLI/packaging | Preserve state/log ownership, rollback, launchd behavior, and requester configuration. |
| Chancery bundle schema, catalog, contract reader, exact-ID resolver, or directory installation | Chancery | Preserve read-only behavior, failure isolation, exact basis, explicit gaps, complete installed inventory, and provider-owned selectors; do not introduce semantic matching or a product runtime dependency. |
| A product's provider scope, normalized promise, capability, operation, or substantive reliance | Owning product | Stage the version-matched bundle with its release, scope inventory completeness meaningfully, keep reliance distinct from documentation dependencies, validate it in product CI, require the complete root CI to accept the fifteen-provider source graph, and update only that product's Chancery selectors. |

## Guarded change playbooks

### Routine Nucleus patch

1. Decide whether the patch changes any public semantic, store schema, harness
   support, operator action, or requester obligation. Update this manual and the
   exact contract documents when it does.
2. Run the Nucleus product quality gate from the Cell checkout:

   ```sh
   cd /Users/joey/rust/cell
   ./nucleus/ci.sh
   ```

   That gate covers only the six Nucleus packages and Nucleus's shell and
   packaging checks. A root aggregate CI run does not replace the product
   gate.

3. If publishing a release, run `nucleus/release.sh` only from clean `main` that
   exactly matches `origin/main`. The script changes the Nucleus workspace
   version, commits, creates a `nucleus-vMAJOR.MINOR.PATCH` tag, and pushes;
   invoking it is a publication action, not a build step.
4. Quiesce requesters if replacing the daemon could lose active work.
5. Deploy matching CLI and daemon candidates with the exact Codex executable:

   ```sh
   /Users/joey/rust/cell/nucleus/packaging/macos/deploy-user.sh \
     --binary /Users/joey/rust/cell/target/release/nucleus \
     --daemon /Users/joey/rust/cell/target/release/nucleusd \
     --codex /absolute/path/to/codex
   ```

6. The installer stages files, replaces the LaunchAgent, and allows up to two
   minutes for first-start migration, compaction, and health. A failed cutover
   restores captured binaries and service configuration only when the database
   schema did not change. It refuses an unsafe binary-only rollback after a
   schema cutover. Authentication is deliberately excluded from rollback. The
   packaging wrapper also switches the immutable Nucleus release and its
   product-owned Chancery provider selector; a failed service install restores
   those selectors, and Chancery itself is never part of runtime readiness.
7. Verify strict health, a fresh generic job, and each affected requester
   canary before resuming dispatch.

### Exact Codex upgrade

Do not replace the configured Codex executable optimistically. The adapter
rejects any version it has not proved.

1. Inspect the candidate executable, its version, model catalog, generated
   app-server schema, and every method, field, enum, tool, and isolation semantic
   Nucleus consumes.
2. Update the exact adapter version and semantic compatibility tests.
3. Run the full Nucleus quality gate against the candidate.
4. Quiesce requesters and deploy Nucleus with the candidate's absolute path.
5. Confirm health reports that exact executable, harness version, and required
   capabilities.
6. Run a fresh Nucleus job and the affected requester canaries, including a
   deliberate Todo creation, Annals reconciliation, or isolated CRM steward
   revision as applicable.
7. Rebuild affected requesters only if the stable Nucleus types or semantics
   they consume changed.

### Public protocol or client change

1. Decide whether the change is additive within the current protocol or needs
   a new protocol version.
2. Update `nucleus-core`, `nucleus-client`, daemon routes, the runtime contract,
   examples, and HTTP contract tests together.
3. For additive support, deploy the accepting daemon before a requester emits
   the new form.
4. For an incompatible change, retain both versions during migration when
   possible. Otherwise quiesce all requesters for a coordinated cutover.
5. Update requester adapters deliberately. Prove duplicate admission, mailbox,
   domain-success, and failure semantics again.

### Nucleus database schema change

The current store schema is version 2 and has an explicit version-one cutover.
That cutover preserves jobs, attempts, immutable schemas/toolsets, cancellation,
and terminal state, but deliberately discards the old mixed log and historical
answered mailbox rows. It refuses to run while a pending requester tool call
still has a nonterminal owning job and attempt; stale pending rows whose owner
is terminal are discarded with the other mailbox history. It then creates the
four-column harness-output ledger and commits the new tables with transitional
`user_version=1000002`, meaning version 2
compaction is still pending. Every restart recognizes that durable marker and
retries `VACUUM` plus a truncating WAL checkpoint; only after both succeed does
it publish `user_version=2` and allow startup to continue. This reclaims both
the dropped main-database pages and the migration WAL while the daemon remains
open. Publishing the completion marker can leave at most its single bounded WAL
frame. A vacuum or checkpoint failure remains pending and is surfaced again on
the next startup rather than being masked by launchd restart.

Version-one binaries cannot read schema version 2. The installer records the
pre-install schema and refuses to restore old binaries if a replacement daemon
has changed it. Recovery across that boundary requires an explicit matching
database and binary pair; credentials remain forward-only and separate.

For version 3 or any later change:

1. Implement explicit, incremental migrations from every supported prior
   version.
2. Add a real old-schema fixture containing representative operational state
   and output atoms.
3. Prove migration is transactional and test any post-commit maintenance.
4. Quiesce requesters, require zero pending requester calls, stop Nucleus, and
   take a consistent pre-migration backup when rollback or retained history is
   required.
5. Define whether the old binary can read the migrated database. If not,
   rollback requires both the old binaries and the pre-migration database;
   installer binary rollback alone is unsafe.
6. Validate operational state, output ordering, mailbox foreign keys, derived
   reads, file compaction, and requester canaries.
7. Keep credential restoration separate. A database rollback must not replace a
   newer Nucleus-owned credential.

### Requester schema, toolset, prompt, or permission change

- Publish a new schema ID or toolset version when a registered meaning changes;
  never mutate an old registration.
- Keep old result decoders for historical jobs.
- Treat prompt, model, reasoning, timeout, workspace, builtin-tool, and launch
  context changes as invocation behavior changes owned by the requester.
- Use a new job ID for a new attempt. An existing job ID can only rediscover the
  byte-equivalent request.
- Re-run domain acceptance tests and canaries. Nucleus health proves capability,
  not that the requester's domain rule is correct.

### Authentication or service-ownership change

1. Pause or prevent every requester and let credential users settle.
2. Identify the one authoritative credential home before moving anything.
3. Preserve private directory and file modes, the exclusive login/session
   barrier, and the serialized canonical mutation boundary. Never distribute a
   managed refresh token to job homes or let workers copy authentication back.
4. Never make an installation rollback restore older authentication bytes.
5. When moving authority from another system, securely transfer the current
   credential after the old writer is stopped or perform attended login in the
   new authority.
6. Verify account, strict health, refresh behavior, and requester canaries before
   resuming work.

## Diagnosis and recovery

| Observation | Meaning | First action |
| --- | --- | --- |
| `nucleus health` cannot connect | The socket or daemon is unavailable, or the configured path is wrong. | Run `nucleus service status`, inspect the LaunchAgent and Nucleus stderr log, and avoid requester fallback. |
| Health is degraded with an unsupported harness | The configured Codex executable no longer matches the proved adapter. | Restore the proved executable or complete the exact Codex upgrade playbook. |
| `model_auth_unavailable` | The Nucleus-owned credential or account read failed. | Quiesce, perform attended login, verify account and health, then canary. |
| `authentication_busy` | Another canonical account, refresh, or login operation owns the short exclusive credential boundary. | Wait or use the requester's documented bounded wait; do not replace credentials. |
| Job is `waiting_on_requester` | A durable dynamic tool call has not received its requester-owned result. | Inspect pending calls and the requester process/domain state. Restart the requester if it supports mailbox recovery; do not invent a result manually. |
| Attempt is `lost` | Nucleus restarted while the harness attempt was unfinished. | Inspect domain state first. Let the requester decide whether and how to create a new attempt. |
| Nucleus job failed after a domain commit | Runtime completion failed after the requester established success. | Preserve the domain result, correlate the Nucleus failure for diagnostics, and do not repeat the mutation. |
| Nucleus job completed without the required domain record | The model turn completed but the requester did not establish domain success. | Follow requester failure policy; Nucleus completion alone is insufficient. |
| Installation reports rollback | Candidate health or cutover failed and captured program/service artifacts were restored because the database schema was unchanged. | Verify the restored service and forward-only authentication; inspect diagnostics. |
| Installation refuses rollback after a schema change | The new database cannot safely be opened by the captured old daemon. | Keep the candidate binaries, inspect the startup error, and use an explicit matching database/binary restore only if recovery requires it. |
| State or logs grow continuously | Nucleus has no automatic output pruning or host-log rotation. | Measure both paths, apply private host-log rotation, and plan supported output retention rather than deleting rows. |

For Annals failures, use its status, pause, interruption, and bounded retry
procedures. Never move failed envelopes back into the queue or edit their
receipts. For Todo, inspect the Todo database/result first; a committed creation
wins over later runtime failure. Detailed recovery policy remains in each
requester. For CRM, inspect the queued run and revision first; a committed case
revision remains success even when the steward job later fails, while a
terminal job without that revision is not CRM success.

## Canaries and resumption

Use layered proof after a shared change:

1. **Service proof:** matching CLI/daemon versions, `nucleus service status`,
   strict health, expected harness, and authenticated account.
2. **Generic runtime proof:** submit a fresh smoke job with a new job ID. The
   checked-in example is a template; reusing its existing ID only exercises
   idempotent lookup rather than a new attempt.
3. **Requester proof:** exercise the requester's actual domain result and verify
   its database, not only the Nucleus terminal state.
4. **Observation proof:** locate the job through requester identity and read its
   ordered harness-output atoms.
5. **Resumption:** clear only the requester-owned pause or gate that was set for
   the operation. Do not remove Annals maintenance files manually.

An Annals integration canary creates a real examination and reconciliation
record even without `--apply`; choose the work deliberately and inspect the
result. A `todo new` canary creates a real concern and pending routing proposal,
not an accepted `tN`; choose a real source and direction or use an isolated Todo
database. Do not leave unexplained canary domain records.
Sending or previewing Todo's email digest does not exercise Nucleus and is not
a requester canary. A Weaver canary replaces the selected narrative's current
outputs; use a deliberate fixture or repository and validate the five persisted
files rather than relying on Nucleus completion.
For Krisis, canary only after its write-once baseline and exact Annals decisions
library identity exist. Create one deliberate root turn whose user message
explicitly settles a choice, then verify binary authority coverage, the exact
source span, deterministic account and outbox bytes, correlated
`krisis/decision-account-classification/1` job, Annals acceptance receipt, and
one matching decision-feed event. A turn need not change a file to qualify. Use
isolated Krisis and Annals decisions-library state when durable canary records
would be misleading.
Conversations inspection alone is read-only and is not a Nucleus canary.
For Semantics, register only an intended folder at the current Annals decisions
feed watermark. A seed proves repository replay without creating a Nucleus job.
A requester canary requires a newly accepted decision account after activation:
verify the Annals event identity, intake receipt, exact grounding, and committed
semantic revision, not merely a terminal Nucleus job. Do not fabricate a durable
decision account or semantic revision solely to make a canary green; use
isolated databases when no real project decision is available.
For Geste, initialize state explicitly after deployment, then create a real
episode and prove search, historical show, report, and graph from the installed
database. Do not fabricate a legacy Decisions event merely to label a
settlement verified. If the intended source admission is not yet available, retain a
provenance-bearing Todo for the later self-episode.
For CRM, use an isolated database and deliberate `crm tell` content. Verify the
durable queued run, one committed case revision, the correlated `crm` Nucleus
job and `crm/case-steward/1` toolset, and prominent advisory rendering on every
consumption surface. Also prove that the advisory does not change command
success and that Nucleus completion without a committed revision is not CRM
success.
For Pratica, use an isolated database and a deliberate integration whose source
snapshots were obtained through their owning public contracts. The acceptance
canary brokers only the CRM terms derived from “Review CRM data model concerns”:
seal the exact bilateral agreements, retain a composition review, and verify
Pratica domain records plus correlated Nucleus jobs. It must create no CRM
source, database, migration, API, UI, deployment, or release. A terminal job,
unsealed offer, or advisory review alone is not a requester canary.

## Where facts and changes belong

Use these placement rules to keep the manual current and small:

- **Operator manual:** current shared topology, authority boundaries,
  compatibility axes, safe ordering, backup, recovery, and canary obligations.
- **Todo:** an unimplemented actionable outcome or researched follow-up.
  “Implement pruning” may be a todo; “Nucleus currently does not prune” is
  current operator truth.
- **CRM:** employment-relationship cases, supplied content, case revisions,
  evidence, steward-run state, and conspicuous nonblocking advisories. It is
  not outreach authority or a scheduler.
- **Annals:** retained source material and evidence-grounded conceptual
  knowledge. It may retain released documentation, but it is not the sole
  editable runbook.
- **Geste:** bounded historical work episodes, their authored interpretation,
  applicability, source basis, and precedent relations. It is not current
  procedure, source truth, or a decision authority.
- **Pratica:** exact integration offers, assent, steward bases, agreement seals,
  and separate composition/conformance reviews. It is not target implementation,
  exhaustive concern discovery, source truth, or deployment authorization.
- **Component documentation:** exact Nucleus protocol, Todo creation behavior,
  Annals corpus and inbox behavior, or Annals Usage accounting.
- **Chancery provider bundle:** current, version-matched provider promise
  scope, normalized outward-boundary claims and substantive reliances, user
  capability cards, detailed manuals, and adaptive cross-system operations.
  The owning product controls its claims; Chancery controls schema, discovery,
  deterministic resolution, exact basis, and gap classification. An operation
  describes choreography but does not execute it, and a resolved dossier is
  not runtime readiness or implementation proof.
- **Code, schema, migration, and tests:** behavior the software must enforce.
  Documentation does not enforce idempotency, compatibility, or rollback.
- **Code comment:** a narrow, non-obvious local invariant or race, paired with a
  test where practical.
- **Git history:** what happened. Do not turn the manual into a changelog or
  decision diary.

Update this manual in the same change whenever public compatibility, persistent
state, authentication or service ownership, deployment order, requester
boundaries, operator action, recovery, or canary obligations change. Prefer
proof commands and versioned authorities over “last verified” dates. Keep
networked cross-product canaries in the release procedure rather than folding
them into deterministic product CI.

## Reference map

The absolute checkout paths below are intentional: this file is also embedded
in `nucleus manual`, so its links must work when the command is run from any
directory.

### Nucleus

- [README](/Users/joey/rust/cell/nucleus/README.md): build, install, readiness,
  storage warning, and smoke entry points.
- [Runtime contract](/Users/joey/rust/cell/nucleus/docs/runtime-contract.md): exact
  request, harness, mailbox, output ledger, HTTP, authentication, and security
  semantics.
- [Annals and Todo handoff](/Users/joey/rust/cell/nucleus/docs/annals-todo-handoff.md):
  current requester ownership and shared acceptance checks.
- [`examples/`](/Users/joey/rust/cell/nucleus/examples): complete request, schema,
  and toolset templates.
- [`packaging/macos/deploy-user.sh`](/Users/joey/rust/cell/nucleus/packaging/macos/deploy-user.sh):
  guarded user-service deployment.
- [`release.sh`](/Users/joey/rust/cell/nucleus/release.sh): publication workflow; it
  commits, tags, and pushes.

### Annals

- [Documentation index](/Users/joey/rust/cell/annals/docs/README.md)
- [Architecture](/Users/joey/rust/cell/annals/docs/architecture.md)
- [System installation and recovery](/Users/joey/rust/cell/annals/docs/system-installation.md)
- [Consumption telemetry](/Users/joey/rust/cell/annals/docs/telemetry.md)

### Todo

- [Documentation index](/Users/joey/rust/cell/todo/docs/README.md)
- [Architecture](/Users/joey/rust/cell/todo/docs/architecture.md)
- [CLI contract](/Users/joey/rust/cell/todo/docs/cli.md)
- [User-owned installation](/Users/joey/rust/cell/todo/docs/system-installation.md)

### CRM

- [Documentation index](/Users/joey/rust/cell/crm/docs/README.md)
- [Architecture](/Users/joey/rust/cell/crm/docs/architecture.md)
- [CLI contract](/Users/joey/rust/cell/crm/docs/cli.md)
- [Data model](/Users/joey/rust/cell/crm/docs/data-model.md)
- [User-owned installation](/Users/joey/rust/cell/crm/docs/system-installation.md)

### Weaver

- [Documentation index](/Users/joey/rust/cell/weaver/docs/README.md)
- [Architecture](/Users/joey/rust/cell/weaver/docs/architecture.md)
- [CLI contract](/Users/joey/rust/cell/weaver/docs/cli.md)
- [User-owned installation](/Users/joey/rust/cell/weaver/docs/system-installation.md)

### Email

- [README](/Users/joey/rust/cell/email/README.md)
- [CLI contract](/Users/joey/rust/cell/email/docs/cli.md)
- [User-owned installation](/Users/joey/rust/cell/email/docs/system-installation.md)

### Conversations

- [Documentation index](/Users/joey/rust/cell/conversations/docs/README.md)
- [Architecture](/Users/joey/rust/cell/conversations/docs/architecture.md)
- [CLI contract](/Users/joey/rust/cell/conversations/docs/cli.md)
- [User-owned installation](/Users/joey/rust/cell/conversations/docs/system-installation.md)

### Krisis

- [Documentation index](/Users/joey/rust/cell/decisions/docs/README.md)
- [Architecture](/Users/joey/rust/cell/decisions/docs/architecture.md)
- [CLI contract](/Users/joey/rust/cell/decisions/docs/cli.md)
- [Data model](/Users/joey/rust/cell/decisions/docs/data-model.md)
- [User-owned installation](/Users/joey/rust/cell/decisions/docs/system-installation.md)

### Semantics

- [Documentation index](/Users/joey/rust/cell/semantics/docs/README.md)
- [Architecture](/Users/joey/rust/cell/semantics/docs/architecture.md)
- [CLI contract](/Users/joey/rust/cell/semantics/docs/cli.md)
- [Data model](/Users/joey/rust/cell/semantics/docs/data-model.md)
- [User-owned installation](/Users/joey/rust/cell/semantics/docs/system-installation.md)

### Geste

- [Documentation index](/Users/joey/rust/cell/geste/docs/README.md)
- [Architecture](/Users/joey/rust/cell/geste/docs/architecture.md)
- [CLI contract](/Users/joey/rust/cell/geste/docs/cli.md)
- [Data model](/Users/joey/rust/cell/geste/docs/data-model.md)
- [User-owned installation](/Users/joey/rust/cell/geste/docs/system-installation.md)

### Pratica

- [Documentation index](/Users/joey/rust/cell/pratica/docs/README.md)
- [Architecture](/Users/joey/rust/cell/pratica/docs/architecture.md)
- [CLI contract](/Users/joey/rust/cell/pratica/docs/cli.md)
- [Data model](/Users/joey/rust/cell/pratica/docs/data-model.md)
- [User-owned installation](/Users/joey/rust/cell/pratica/docs/system-installation.md)

### Chancery

- [Documentation index](/Users/joey/rust/cell/chancery/docs/README.md)
- [Architecture](/Users/joey/rust/cell/chancery/docs/architecture.md)
- [CLI contract](/Users/joey/rust/cell/chancery/docs/cli.md)
- [Provider manifest](/Users/joey/rust/cell/chancery/docs/manifest.md)
- [User-owned installation](/Users/joey/rust/cell/chancery/docs/system-installation.md)

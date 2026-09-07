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
| CRM | Employment-relevant people, opportunities, and contemplated contact should be retained as evidence-grounded cases, or reusable career profile material should be stored. | Its local SQLite library, mutable Markdown profile entries, queued steward runs, immutable case revisions, evidence, and advisory review notes. | Sending or authorizing outreach, scheduled intake, treating an advisory as a gate, or storing CRM domain state in Nucleus. |
| Cast | Previously unknown employers and job postings should be discovered and monitored through ordinary HTTP. | Private discovery records, company/job identities, extracted evidence, source coverage and freshness, local request budgets, query configuration and consistent exports. | Personal selection, CRM stewardship, application packets, email, application submission or agent execution. |
| Platter | A retained Cast opportunity needs a private paragraph brief and resume with only Jackson bullets tailored, or a dated edition should be previewed and explicitly sent. | Captured posting/career/template inputs, accepted Nucleus stages, fixed-template rendering, opportunity exclusion, frozen editions and recorded send outcomes. | Discovery, CRM editing, changes to fixed resume content, employer contact, applications, or an installed recurring delivery service. |
| Annals | Immutable source material should be retained or reconciled with an evidence-grounded conceptual corpus, or that corpus should be searched or explored. | Each selected physical library's retained works, concepts, evidence, reconciliations, revisions, source deliveries, inbox policy, and domain recovery. | An action backlog, casual notes or preferences, agent-process supervision, cross-library federation, or account telemetry. |
| Weaver | Authored repository inputs should become the current five-stage public-facing narrative outputs. | Current-run admission, stage order and input snapshots, repository output writes, validation, cancellation intent, and recovery. | Publishing, editing a public profile, treating generated text as factual authority, or general job orchestration. |
| Email | A plain-text email, optionally with authorized local file attachments, should be sent to the single fixed recipient. | The synchronous frozen Resend request and its fixed sender and recipient contract. | Drafting without sending, arbitrary recipients, remote attachment URLs, or agent execution. |
| Conversations | Codex tasks on this Mac should be listed, inspected, or searched. | A read-only normalized view over the normal user's Codex App Server. | Decision classification, durable projections, live-process supervision, or Nucleus's isolated job history. |
| Krisis | Attributable decisions in completed root user turns should be identified and delivered as immutable accounts to the dedicated Annals decisions library. | The observation baseline and coverage, bounded classification, source anchors, account projection, durable outbox, Annals acceptance receipts, and recovery. | Judging truth, importance, applicability, enactment, current force, review state, supersession, retaining the canonical account library, or sending a digest. |
| Semantics | A registered project folder's authoritative terminology and semantic history should be explored or maintained from accepted accounts in the dedicated Annals decisions library. | Project registration and routing, stable concept identities, append-only semantic revisions and evidence, decision-feed intake, Nucleus reconciliation, and recovery. | General documentation generation, unregistered folders, source-code behavior, transcript storage, or interpreting Annals retention as semantic truth. |
| Usher | A Cell checkout's declared membership should be reported or required in CI, or Usher's own installation should be verified or recovered. | Deterministic recognition of product identity, Semantics participation, and Chancery introduction evidence; a separate installer owns Usher release and selector operations. | Proving other products' actual registration or installation, relationship quality, readiness, or other inter-system affairs. |
| Nucleus | A local application needs constrained agent execution, or shared execution, authentication, compatibility, job history, deployment, or requester integration must change. | Admission, the portable invocation contract, eight-slot harness supervision, single-authority credential coordination, cancellation, exact harness-stdout observations, and the durable dynamic-tool mailbox. | Domain success, project registration, workflow graphs, requester retry policy, or reporting materializations. |
| Annals Usage | Annals-attributed model consumption, account allowance, login, or the Annals-to-Nucleus execution path must be inspected. | Live calculation over Annals attribution and Nucleus output atoms, plus Annals-facing budget and diagnostic commands. | Nucleus runtime authority, durable reporting projections, Codex credential storage, Annals corpus success, or general job orchestration. |
| Codex | Nucleus needs an inspected harness and account protocol implementation. | Its executable and app-server behavior. | Requester domain policy or a second credential authority for Nucleus jobs. |

Typical routing examples:

- “Leave this concern for later” is Todo work.
- “Retain this possible employment connection and assess the case for contact”
  is CRM work.
- “Discover new employers and refresh their public job postings” is Cast work.
- “Incorporate this report into what we know” is Annals work.
- “What does the corpus say about predicate locking?” is an Annals query.
- “Build the current public-facing narrative” is Weaver work.
- “Email me this update” is Email work.
- “What does this project mean by grounding?” is a Semantics query when that
  folder is registered.
- “What does Krisis promise an account consumer?” starts with Chancery
  discovery and exact-ID promise resolution.
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
cast --version
email --version
conversations --version
krisis --version
semantics --version
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
Semantics worker ---/
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
Cast SQLite <------- Cast HTTP adapters --> search providers and careers sources
Weaver outputs <---- Weaver's detached repository worker
Email -------------> Resend
Conversations ------> normal-user Codex app-server
Codex Stop hook ----> Krisis SQLite observation queue
Krisis observer ----> Conversations
Krisis observer ----> Annals decisions-library acceptance
Annals decision feed -> Semantics --> Conversations exact cwd
Semantics -----------> registered project semantic repositories

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

CRM is a short-lived local CLI, SQLite case and profile library, and bounded
Nucleus requester. `crm profile` stores and reads mutable Markdown entries
locally; it launches no Nucleus job, changes no case, and supplies no automatic
steward context.
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

Cast is an ordinary Rust discovery CLI with its own private SQLite state. It
searches configured providers, follows public careers sources, and retains
distilled employer/posting evidence, coverage, freshness and conservative local
budget accounting. It invokes no Nucleus job, CRM steward, browser, packet
builder or email sender. Downstream products consume its supported snapshot
and retain their own selection, application and notification state. Collection
success is the committed evidence and explicit coverage outcome; provider
failure or partial collection cannot establish that a job closed.

Platter is a requester CLI with a maintained installation route and no
automatically activated delivery service. Unchanged prior-installation recovery proves retained state compatibility
without requiring unavailable delivery or rendering dependencies; candidate
verification requires full readiness. New work uses requester program
`platter`; retained predecessor requests keep their exact `job-packets`
identity and bytes. One packet ID
correlates a brief job followed by a resume job, both using exact model
`gpt-5.6-sol` at `max` effort. The runner consumes Cast's supported export,
fetches full employer posting evidence, and captures CRM profile entries
through supported list/read operations. Each model independently chooses
which entries to inspect through `list_career_entries` and
`read_career_entry`, served from the same retained career snapshot. Separate
CRM reads are not a transactional snapshot; capture checks entry timestamps
and refuses an incomplete list. CRM retains editing authority.

New brief jobs use `platter/brief/2` and
`platter.submit-brief.arguments.v2` to submit `why_it_works`, `role`, optional
`culture` and private `pursue`. Why it works gives one or two direct, confident,
evidence-grounded sentences of at most 45 words. Role states stack,
responsibilities and process in one or two short lines of at most 30 words;
Culture states working norms in one or two short lines of at most 25 words.
Role and Culture do not compare the opportunity with the user's experience.
Culture is omitted when the posting or existing captured material does not
support it, with no extra research or uncertainty note. Displayed section
content totals at most 90 words and contains no caveats, downsides or hedging.
The pursuit criteria remain private. Platter renders labeled blocks into the
existing brief `paragraph` string; retained v1 requests and accepted outputs
remain supported without rewriting them.
Only a worthwhile opportunity proceeds to resume preparation. The
resume model authors only Jackson work-experience bullet text and private
career-entry references. The renderer escapes that text and preserves every
byte outside the original resume's Jackson bullet span. The original
template, captured inputs, exact requests, tool receipts, accepted results,
editable source and PDF remain requester-owned private artifacts. Both jobs
have workspace access `none`, no local execution or web search, and no launch
context. Validated stage submission and successful fixed-template rendering,
not final model prose, establish packet readiness.

Platter freezes at most three previously unsent ready packets in one dated
edition, including exact briefs, copied PDFs and attachment digests. New
editions separate headers and brief blocks with whitespace and omit the
packet-count footer; existing frozen editions remain unchanged. It
rechecks posting evidence before creating a new edition and reserves selected
opportunities against later editions. Explicit `send` validates retained
attachment digests and invokes Email with a stable edition key; only an
`Accepted ID` receipt records submission acceptance. Uncertain sends remain
held, with no blind resend or current reconciliation command. Accepted stages
survive later failures; there is no automatic replacement Nucleus job or
direct-Codex fallback. No live delivery proof follows from this implementation.

The separate `preview DAY --ad-hoc RUN_ID` / `send DAY --ad-hoc RUN_ID` path
supports explicitly authorized test emails from retained completed packets.
It reads the packet database without opening it for writes and performs no
Cast, CRM, posting HTTP, Nucleus or rendering call. The `[TEST]` payload,
copied PDFs, hashes, stable key and receipt live under private
`ad-hoc/RUN_ID/`; normal packet reservations, editions and sent flags remain
unchanged. A repeated accepted test is a no-op, and uncertainty stays held.
Retained posting evidence is not refreshed, so this path supplies no new
vacancy-freshness proof and consumes no Cast API budget.
Optional reviewed paragraph overrides are frozen only in the test occurrence
beside the original accepted brief and source-state hashes; ordinary brief
and resume artifacts remain unchanged. Packet selection may include previously
sent or reserved records because ad hoc test history is independent.

The stored defaults are three packets and 09:00 `America/Chicago`, with no
candidate or token budget. Execution, source and rendering timeouts still
apply. These settings install no Clockwork binding or LaunchAgent and provide
no scheduled activation. `platter-install` provides verified packaging and
Cell coordinated maintenance; a production delivery scheduler and unresolved
stage/send recovery remain separate integration work.

Weaver submits five content-only jobs in order. Its detached interactive-lineage
worker owns repository reads and atomic Markdown output writes; Nucleus and the
Codex process never receive repository filesystem authority. Weaver success
requires its persisted outputs and mechanical validation, not merely completed
Nucleus jobs.

Email is a synchronous CLI that sends plain-text messages directly to Resend.
It creates no Nucleus job, uses no Nucleus authentication, owns no daemon or
domain database, and does not depend on Nucleus health.
The source attachment extension accepts repeatable `--attach PATH` for local
regular files, including resume PDFs. It captures each file once before
network submission; attachment order, basenames and exact encoded bytes belong
to the same frozen request and idempotency key throughout bounded retries.
Only basenames and contents are transmitted, never local source paths. An
upstream caller owns authorization and must preserve exact files for any later
same-key invocation. `Accepted ID` means Resend accepted the message, not Gmail
receipt. An older installed Email release without attachment contract 4 cannot
serve attachment sends. For one authorized ad hoc test, Platter accepts an
absolute `--email-executable` override without changing normal configuration.
A private temporary copy of Email's existing credential wrapper may select
the tested source binary by replacing only its two payload-executable paths.
Preserve disabled tracing, secret loading, scrubbed environment and stdin
handling; never write the key to an argument, file or output. This selects a
tested candidate for that invocation without deploying Email. Remove the
temporary wrapper afterward and retain the isolated test occurrence/receipt.

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

Nucleus, Annals, Annals Usage, Todo, Chancery, Weaver, Email, Conversations,
Krisis, Semantics, Clockwork, CRM, Usher, Cast and Platter share the Cell source
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
7. **A CRM advisory is not a gate.** It remains conspicuous wherever the case
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

CI captures each execution's stdout and stderr in a private broker log outside
the checkout. The default output is one conclusive result, with progress at
most once per minute for long waits or execution. A failure includes at most
4 KiB of diagnostics and a log path available to both the original caller and
joined callers. Failed logs are capped at 8 MiB with explicit truncation and
follow the journal's 14-day/256-execution terminal retention; successful logs
are discarded. `--verbose` requests detailed output, while
`--verbose-receipt` preserves the full machine receipt used for candidate
staging. Presentation options do not change execution identity or exit codes.

The root `./ci.sh` first runs `pipeline/test.sh` as a brokered light preflight,
then records one exact source key and runs a candidate Usher
recognition check in the broker's heavy lane, including for selected-product
runs. It passes the expected source key to that check and each independently
scheduled product gate, and rejects the plan with exit 75 if the worktree
changes. A complete run then rebuilds Chancery for that same candidate and
validates the integrated source graph.
This aggregate evidence does not merge product release authority or turn one
product gate into another's gate.
Root CI suppresses child success summaries and reports the selected scope once.
Use `./ci.sh --verbose [PRODUCT...]` for detailed output or a product's public
`ci.sh --verbose` for that gate. The private `pipeline/check.sh` preflight body
does not invoke Cargo.

Usher reads every `pipeline/products/*.sh` descriptor as literal data and
requires an unambiguous product identity/root, an exact root Semantics marker,
and recognizable product-owned Chancery introduction material. Its report
keeps missing, invalid, and unassessed evidence visible. A declaration does not
prove Semantics registration, vocabulary quality, Chancery installation, full
bundle validity, capability completeness, or runtime readiness. Other systems'
relationships are outside Usher's jurisdiction. Root CI's source check is
read-only; it never queries or registers live services. See
[`usher/README.md`](../../usher/README.md) for exact evidence rules and exit codes.

Each product release entry point uses its checked-in descriptor but remains a
separate release unit. It holds one Git-common-directory publication lock
through its release build, commit, tag, and atomic push, and rechecks
the expected `origin/main` revision and absent tag before publication. A
release command still requires separate explicit authority; CI never invokes
one.

Release and deployment preparation use `deployment/build.py` to build only
selected production packages and binaries in one release-profile Cargo batch,
then seal independent product candidates in parallel. They assume relevant
development CI has already passed, without rerunning CI or requiring its
receipt. Tests, formatting, Clippy, documentation builds and generator checks
remain in development CI. The builder uses a separate persistent Cargo target
and file lock per logical Git repository, with at most eight Cargo jobs by
default. Its content-addressed cache verifies source/build inputs and executable
hashes and versions before reuse. Git HEAD is excluded from the build key, so
the same version-updated source bytes can reuse artifacts after publication
commits them. A build receipt is not CI evidence.

Deployment remains product-owned. Every deployer takes its existing product or
update lock before the shared Chancery catalog-writer lock and holds the catalog
lock through selector cutover, smoke, and rollback. Generated selector-only
deployers for Conversations and CRM stage immutable bytes
before the short catalog critical section, publish command and provider through
one atomic product `current` selector, and reject a changed observed or explicit
`--expected-current absent|releases/HASH` precondition. Stateful deployers keep
their product-specific quiescence, migration, service, and recovery logic and
are conservatively globally conflicting for orchestration. Catalog presence,
CI success, and release preparation grant no deployment authority.

Usher uses a separate Rust `usher-install` executable backed by the shared
`cell-install` library. The recognition command and library retain their
read-only declaration boundary. Release preparation builds and seals both
`usher` and `usher-install`; direct installation takes the exact recognition
binary and provider bundle, while coordinated deployment invokes the sealed
installer's version-one JSON adapter. There is no Usher Python adapter or
checked-in shell installer. This migration applies only to Usher.

Usher's `cell-install-v1` release retains `bin/usher`, `bin/usher-install`,
the exact recovery executable `package/install`, and its provider bundle under
one content identity with a file-hash/mode manifest. Both public commands and
the provider follow one atomic `current` selector. The installer checks an
observed or explicit expected selection, acquires the product lock before the
catalog writer lock, and refuses foreign or tampered ownership. It exposes
read-only `inspect`, exact-candidate `verify`, and retained `verify-release`
operations, plus deliberate `recover --release ABSOLUTE_RELEASE_DIR`.
The retained Rust recovery executable also restores supported legacy releases,
detaching the public installer selector that those releases lack; the same
retained executable can later reselect its Rust release. See the version-two
[`usher.install.operate` manual](../../usher/chancery/manuals/install-operate.md)
for commands and recovery boundaries. No database, semantic registration,
worker, schedule, or other product runtime is changed.

### Selection-only Cell deployment

[`deployment/README.md`](../../deployment/README.md) defines the coordinator and
version-one product adapter boundary. From a Cell checkout, `./deploy.sh plan
SYSTEM...` previews committed declarations; `./deploy.sh SYSTEM...` runs the
selected deployment in the foreground. Product-owned dependencies order selected
releases and discover affected installations to hold; they never silently
upgrade an unselected system. Annals includes Usage, and `decisions` aliases
`krisis`.

The coordinator fixes local committed `main`, prepares a separate worktree,
and asks the shared release builder for all selected products in one batch.
The existing schema-one candidates carry exact source/artifact identity, with
the commit binding and a separate build receipt checked before maintenance.
Sealed build bundles are reusable across invocations; the mutable Cargo target
is not the deployment artifact store. The legacy `ci.sh --stage-candidate` interface still
runs the full gate when explicitly requested. Deployment itself does not run
CI, require a passed CI receipt, change versions, commit, tag or push releases.

The foreground host process owns preparation and ordering. Product adapters own
configuration, dependencies, maintenance, migrations, service lifecycle,
readiness and recovery. Temporary source, candidates, logs and operation state
live under `~/Library/Application Support/Cell/deployments/active`. One host lock
is inherited by the worker and its children; product and catalog locks also
protect coexistence with direct deployment commands.
Completed build bundles and the release target persist separately under the
Git common directory's `cell-release-cache` (or `CELL_RELEASE_CACHE_DIR`).
Deployment workspace cleanup preserves this cache, which has no automatic
pruning.

Nucleus, Annals, Krisis, Semantics, CRM, Todo, Weaver and Platter expose durable holds
owned by the deployment identity. Holds block new admission while existing work
settles, survive process exit and do not expire. Releasing one owner preserves
other owners and pre-existing operator pauses. Product status includes durable
domain work; an empty process list alone does not establish a drain.

After all affected products are held and drained, the coordinator applies the
selected candidates in dependency order and checks their installed identity and
ordinary service readiness. Nucleus reports `acceptingJobs=false` while held;
`maintenance health RUN_ID` checks its sole matching owner, drained work,
authentication and exact supported harness. Deployment creates no model jobs or
synthetic domain records. After every readiness check passes, requester holds
are released, then Nucleus is released last.

Completed failures attempt product-owned recovery within the same invocation.
Every affected product must establish a coherent prior or candidate installation
before any hold is released. An uncertain apply is never repeated blindly;
unsafe recovery retains the product's hold and reports its owner for supported
operational recovery. Program selectors alone cannot establish database rollback
or restore consumed authentication. An uncertain Nucleus apply requires its
supported service recovery procedure because matching command files and health
do not establish that an older resident daemon was replaced.

The coordinator prints one final JSON result on stdout after recovery and
cleanup. It includes recovery, maintenance, and cleanup disposition alongside
the existing source, product, run, and exit identities. A recovered deployment
still returns failure, with released holds reported explicitly. Unproved hold
or release outcomes remain uncertain rather than being inferred from a missing
ledger flag. Installation success remains distinct from cleanup failure.
`--verbose` enables detailed progress on stderr; ordinary long-running progress
is limited to once per minute. Original and recovery failure evidence is
collected before deleting temporary logs, with one shared 4 KiB diagnostic
allowance and visible truncation. Adapter data objects are not terminal dumps.

Once the worker exits, the parent reacquires the host lock, unregisters the
worktree and deletes its temporary files, including completed failure logs.
Surviving children prevent cleanup until they release that lock. The next
invocation clears stale inactive workspace under the same lock. There is no
retained deployment history or public `status`, `wait`, `resume` or `recover`
command. Product-owned holds and database recovery backups remain operational
state until resolved; ordinary domain and Nucleus execution history is unaffected.
Release-build caching does not retain deployment operation logs or recovery history.

Successful coordinated deployments also remove unreferenced installed release
directories and previous-release selectors. Current releases and releases still
pinned by configuration, schedules or running processes remain available.
Cleanup failure is reported without undoing the completed deployment. Direct
product deployers retain their existing rollback behavior; this pruning is part
of the coordinated Cell command.

The cleanup reader accepts legacy complete Clockwork binding arrays and expands
version-two selection pages until the inventory is complete. Unknown or incomplete
inventories stop cleanup before deletion.

### Migration to coordinated deployment

The initial migration must install maintenance-capable binaries through the
existing product deployment procedures. A hold created by a candidate binary
does not fence a currently running older binary that never checks that hold.
First stop each product's scheduled admission through its existing owner,
preserve the original enabled/paused state, and settle existing domain work and
Nucleus jobs. Then install the compatible foundation and requester binaries,
verify their new maintenance interfaces, and restore exactly the prior schedule
and pause state. Do not infer a successful migration from catalog presence.

An attended handoff may keep the original scheduler and operator gates closed
while releasing its temporary holds to the first normal coordinated run. That
run performs the final installation and requester verification. Restore the
captured external gates only after it succeeds; its temporarily disabled
schedule baseline must not replace the original operator intent.

Thereafter a names-only run owns the operational sequence. Selector-only products
are suitable first pilots; a Nucleus/requester selection exercises affected-only
holds, readiness and restoration. The ordinary stateful adapters
require a configured installation. New databases, authentication provisioning,
new schedule policy, account changes and domain imports remain their documented
explicit operations. The CRM coordinator adapter composes the supported
`migrate --backup PATH` command under the drained hold. Actual migration backups
remain in the CRM application-support directory for database recovery; current-schema
deployments create none. The standalone generated
CRM deployer still changes only program/provider selectors.

### Standard installed paths

```text
~/.local/bin/nucleus
~/.local/libexec/nucleusd
~/.local/bin/email
~/.local/bin/conversations
~/.local/bin/krisis
~/.local/bin/semantics
~/.local/bin/clockwork
~/.local/bin/crm
~/.local/bin/cast
~/.local/bin/usher
~/.local/bin/usher-install
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

~/Library/Application Support/CRM/
  crm.db
  install/

~/Library/Application Support/Cast/
  install/

~/.local/share/cast/
  cast.sqlite3

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

Platter's fresh state defaults to `~/.local/share/platter`. A sole predecessor
`~/.local/share/job-packets` directory remains the default in place; if both
exist, require an explicit global `--state-dir`. No rename moves artifacts or
rewrites original templates, packet IDs, exact stage requests or send receipts.
Back up the database, configuration, original template, packet directories and
frozen editions together while the runner is inactive.

Its `platter-install` packages the runtime, installer and provider under
`~/Library/Application Support/Platter/install/releases/HASH`; owned command
and provider selectors follow one verified current release. Mutating install
and recovery operations run through Cell coordinated maintenance. They verify
schema 1 and existing original-template state, create a consistent database
backup when applicable and require local dependency readiness. No resume
initialization, packet preparation, Cast collection, model job, email or
schedule activation occurs during installation.

The per-user Platter admission gate covers mutating callers even with custom
state. Maintenance observes both `platter` and `job-packets` Nucleus identities
and live runners. Existing callers finish; after they have stopped, drain
cancels orphaned matching Nucleus jobs without submitting replacement work.
Accepted stages and receipts survive cancellation. Nucleus maintenance includes
Platter in its requester closure; readiness is checked while held before
requester release, with Nucleus released last. Failed recovery retains holds.
See the [Platter installation contract](../../platter/chancery/manuals/install-operate.md)
for commands, prerequisite limits and recovery boundaries. Its
`Semantics-Project: cell` marker declares source participation, not a separately
registered semantic repository or installed provider.

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
binding and process history. Its immutable definition records one explicitly
selected absolute Codex executable, and the release-local observer passes that
path as `CONVERSATIONS_CODEX` instead of discovering another installation at
runtime. Final-cutover doctor uses the same path; interactive Krisis commands
that read Conversations must receive it explicitly because they do not inherit
the observer environment. A pre-cutover Decisions release still owns the two
legacy `decisions/observer` and `decisions/daily-email` schedule projections,
whether Clockwork bindings or older LaunchAgents, which are disabled rather
than renamed during the separately authorized cutover. The
deployer refuses any pre-existing foreign `~/.codex/hooks.json`; it never merges,
overwrites, removes, or trusts one. Codex owns exact-definition review through
`/hooks`. The Krisis
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

CRM owns its content-addressed CLI installation, provider selector, and local
SQLite library. All CRM-owned content is stored as database `TEXT`. Deployment
switches immutable program and provider selectors without adding a daemon or
schedule. `crm tell` queues an asynchronous steward run, which uses Nucleus and
never falls back to a direct Codex invocation. Schema two adds current profile
entries. Existing schema-one state requires explicit `crm migrate --backup
PATH` after stopping new work and settling active workers. Migration creates a
private SQLite-aware schema-one backup and adds the empty table transactionally;
the standalone generated deployer never migrates state. The coordinator's
CRM-owned adapter invokes this same migration under its drained hold before
switching program selectors. Program rollback to schema one also requires
a separate quiescent database restore and must preserve newer state before
removing it from the active view.

Cast's product-owned installer stages a Rust payload, zsh frontend and matching
provider bundle and switches only program/documentation selectors. It does not
initialize discovery state, perform collection, migrate a database or activate
a scheduler. The frontend sources the user's `~/.zshrc` with output suppressed
and passes only `THEIRSTACK_API_KEY`, `BRAVE_SEARCH_API_KEY`, `HOME`, a fixed
system `PATH` and an explicitly selected `CAST_STATE_DIR` to the payload.
The two keys do not enter Cast configuration, its database or command arguments.
Discovery state defaults to `~/.local/share/cast`; `--state-dir` selects another
private directory. Program rollback and discovery-state recovery remain separate.
The explicit `cast state reconcile-ownership` repair takes the Cast mutation lock
and transactionally corrects ATS tenant ownership and uncertain older JSON-LD
observations. It preserves source/job IDs, paid usage, run history and query
checkpoints; it performs no remote request. Re-export afterward and refresh
uncertain observations through bounded collection. Do not erase state to reset
its budget accounting.
Cast needs no pause to establish Nucleus quiescence because it never calls it.

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

Completed structured output is reconstructed from retained stdout when a job
is read. Its decoder supports the exact API-key and managed-authentication
startup sequences without retaining outgoing requests or sensitive login
responses. A decoder correction can make an old completed answer readable
without changing job state or running the model again. Missing atoms remain a
gap. Requester terminal records and retry policy do not change merely because
Nucleus can now decode an old answer; inspect the owning product before any
new attempt. Test both authentication paths through the daemon job API as well
as the execution adapter, including replay after restart.

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
nucleus jobs status JOB_ID
nucleus jobs wait JOB_ID --timeout 60
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

### Model-facing output

Select output according to the next decision. Catalogs and required terminology
reads remain complete. Selection lists default to 20 and state whether more
results exist; increase `--limit` or use the command's existing cursor. A result
cap is distinct from source completeness. Excerpts are marked and bounded;
a failed read never becomes an apparently complete empty result. `--json`
selects encoding, while explicit commands/flags select additional content.

| System | Default decision view | Complete/detail read |
| --- | --- | --- |
| Chancery | All cards with shared defaults; one operating manual; resolution gaps first | `show ID --full`, full `resolve ID`; `resolve --summary` selects gaps only |
| Semantics | Project identity/path/status/HEAD; complete meanings, status and distinctions | `repository show PROJECT --provenance` |
| CRM | Profile/case selection, newest-first revision summaries, matching excerpts, write receipts | `profile show`, `case show --revision`; full advisories in every consuming view |
| Cast | Company/job/source selection and matching excerpts; status counts, budgets and failures | `company show`, `job show`, `export` with full evidence and coverage |
| Conversations | Bounded metadata and title/message hits with matching excerpts | `show`, `export`; complete selected source reads still required for search |
| Nucleus | `jobs status`; `jobs wait JOB --timeout 60` | `jobs show`, `jobs logs`, `tool-calls pending` |
| Annals | Committed reconciliation receipts and retry counts/halt | `change show`, `inbox retry status EVENT --details` |
| Annals Usage | Live totals and coverage | `report --details` |
| Todo | Bounded umbrella and concern triage | Exact concern/routing/situation/design/umbrella show commands |
| Clockwork | Bounded lists and activation history | Definition/binding show; `history --details` |
| Krisis | Counts and bounded failure IDs/codes | Increase status `--limit`; legacy lifecycle reads retain their existing protocol |
| Usher | `check` counts and all incomplete findings | `report` |
| Email | `Accepted ID` after Resend accepts the submission | Errors remain bounded; acceptance is not final delivery |
| Weaver | Existing focused run state and artifact locators | Existing run/workflow detail and artifact reads |
| CI/deployment | Existing conclusive receipt and bounded failure diagnostics | Existing `--verbose` and diagnostic paths |

Nucleus status identifies the requester, job and current attempt, runtime state,
pending call IDs/names, final-output availability and terminal reason/message.
The client reads mailbox and job state successively, not atomically. Wait emits
one observation with `outcome: terminal|timeout`; timeout does not cancel, retry,
or make a job terminal. Its initial read must finish even for timeout zero;
subsequent polling is bounded by the requested deadline. Read failures remain
errors. Runtime success remains separate from requester-domain success.

These changes version CLI output and the affected Chancery contracts, while
retaining persistent schemas, immutable toolsets, Codex protocol and accepted
account/event exchanges. Rebuild affected Rust clients with their provider.
For any separately authorized rollout, stage the updated reader and coordinated
provider/client set; source validation does not update the installed catalog
or authorize deployment.

### Quiesce before work that cannot tolerate a lost attempt

Nucleus has no global drain mode. Quiescence is established at its requesters:

1. Do not start a synchronous Todo creation, invoke `crm tell`, start a new
   Weaver submission, invoke `krisis observe process`, or start another manual
   requester job.
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
8. Check Nucleus and requester readiness before resuming the Annals inboxes,
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

3. Resume paused dispatch after account and service readiness are established.

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
resuming dispatch. Do not restore old credentials as part of an ordinary
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

### Provider-owned Rust interfaces

Each system owns the Rust types and operations for its exported interfaces.
Use the provider's public library for decoding, validation, supported reads,
and calls. The exported structs and enums define the data shape; there is no
separate interface registry or central domain-model crate. A consumer may
convert imported types into its own local model, but should not reproduce the
provider's wire structs, parsers, SQL, or command protocol.

Existing libraries remain the integration surfaces for Nucleus and
Conversations. The other product crates expose focused `api` modules.
`krisis-api` holds Krisis accounts and the frozen Decisions lifecycle exchange;
`annals-api` holds Annals acceptance, accepted-account feed, and usage-reader
interfaces so those consumers need not depend on the full applications.
General Krisis operations are available through `decisions::api`; general
Annals corpus reads and CLI operations through `annals::api`.

The Annals and Krisis release scripts advance their companion API crate
versions with the owning product and validate them in the same product gate.

Providers also own intentionally partial views. For example, Usher imports
Chancery's introduction view, and Annals Usage imports Annals' receipt and
database projections. Those views preserve the consumer's existing scope and
tolerance of unrelated fields. Full provider validation remains distinct from
decoding a partial view.

Clients use existing transports and preserve their effects, failures, and
wire formats. This ownership change does not normalize raw Codex usage,
consolidate Todo's email transport, or replace Clockwork's shell consumers.
Keep compatibility fixtures and provider/consumer tests with the owning
products, and migrate affected consumers when a supported export changes.

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
the content they can retain. Add backup coverage, release
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
| CRM profile entries, intake, cases, evidence, revisions, advisories, queued steward runs, database, or deployment | CRM | Preserve its bounded Nucleus adapter and prominent nonblocking advisories; Nucleus gains no CRM fields, domain success, scheduling, or retry authority. |
| Cast company/job identity, source adapters, observations, freshness, local budgets, configuration, exports or deployment | Cast | Keep ordinary HTTP collection separate from Nucleus, CRM stewardship and downstream selection/application/email state; preserve evidence and explicit coverage. |
| Platter source capture, constrained resume authoring, stages, editions or send history | Platter | Keep career editing in CRM, discovery in Cast, execution in Nucleus and acceptance transport in Email. Preserve fixed resume content and held uncertain sends; source defaults do not activate a schedule. |
| Annals works, physical-library identity, concepts, evidence, reconciliation, inbox, producer acceptance, decision feed, retry, or corpus migration | Annals | Keep primary and decisions libraries isolated; preserve job correlation and adapter behavior when affected; Nucleus does not gain Annals workflow state. |
| Annals usage attribution, budget display, or diagnostic projection | Annals Usage | Read Nucleus records through the supported interfaces; do not become runtime or corpus authority. |
| Weaver workflow state, stage prompts, repository inputs or outputs, validation, cancellation, recovery, or deployment | Weaver | Preserve its Nucleus invocation and correlation contract; Nucleus does not gain narrative repository authority or retry policy. |
| Email content, delivery, Resend access, fixed addresses, or deployment | Email | Keep the direct Resend path independent of Nucleus; Nucleus gains no email fields, credential, or delivery authority. |
| Codex task enumeration, normalized transcript reads, App Server compatibility, or Conversations deployment | Conversations | Keep it read-only and separate from Nucleus's private Codex home; consumers must not treat persisted status as live-process proof. |
| Decision identification, observation coverage, account projection, source anchors, Annals delivery, or Krisis deployment | Krisis | Preserve exact user authority, deterministic account identity, Annals acceptance receipts, and Nucleus correlation; no downstream consumer gains classification authority and Nucleus gains no decision fields. |
| Project registration, semantic concepts, grounding, revision history, Annals decision-account intake, reconciliation policy, or Semantics deployment | Semantics | Preserve Annals library/event/account identities, exact Conversations cwd routing, both legacy and new cursor histories, and Nucleus correlation; no upstream gains Semantics state or success authority. |
| New portable invocation meaning or HTTP behavior | Nucleus core/client/daemon | Version the public contract, update examples/tests/docs, then update affected requesters in compatible order. |
| Codex executable or app-server semantics | Nucleus Codex adapter | Prove the exact version, deploy Nucleus, and check installed readiness. |
| Nucleus database schema or retention | Nucleus store | Quiesce, back up, migrate and validate, and define database-aware rollback before deployment. |
| Requester tool arguments, result, or definition | Requester plus immutable Nucleus registration | Publish a new schema/toolset version and keep historical decoding. |
| Requester prompt, model, timeout, or permission profile | Requester | Use new job IDs for new attempts, verify health capabilities, and rerun domain acceptance tests. |
| Managed-authentication, canonical-refresh, or attended-login behavior | Nucleus | Quiesce all credential consumers, preserve forward-only authentication, and check account and service readiness. |
| Nucleus service layout or installer | Nucleus CLI/packaging | Preserve state/log ownership, rollback, launchd behavior, and requester configuration. |
| Chancery bundle schema, catalog, contract reader, exact-ID resolver, or directory installation | Chancery | Preserve read-only behavior, failure isolation, exact basis, explicit gaps, complete installed inventory, and provider-owned selectors; do not introduce semantic matching or a product runtime dependency. |
| A product's provider scope, normalized promise, capability, operation, or substantive reliance | Owning product | Stage the version-matched bundle with its release, scope inventory completeness meaningfully, keep reliance distinct from documentation dependencies, validate it in product CI, require the complete root CI to accept the source graph, and update only that product's Chancery selectors. |

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
   <TESTED_NUCLEUS_INSTALL> install \
     --binary <TESTED_NUCLEUS_BINARY> \
     --daemon <TESTED_NUCLEUS_DAEMON> \
     --bundle /Users/joey/rust/cell/nucleus/chancery \
     --codex /absolute/path/to/codex
   ```

6. Use `nucleus-install` from that same sealed candidate. The Rust packaging
   installer uses shared `cell-install-v2` inventories and selector transactions,
   then invokes the existing Nucleus Rust service installer. Public CLI and
   daemon copies remain service-owned so its rollback capture sees the actual
   previous programs. The installer stages files, replaces the LaunchAgent, and allows up to two
   minutes for first-start migration, compaction, and health. A failed cutover
   restores captured binaries and service configuration only when the database
   schema did not change. It refuses an unsafe binary-only rollback after a
   schema cutover. Authentication is deliberately excluded from rollback. The
   packaging wrapper also switches the immutable Nucleus release and its
   product-owned Chancery provider selector; a failed service install restores
   those selectors, and Chancery itself is never part of runtime readiness.
7. Verify strict health and affected requester readiness before resuming dispatch.

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
6. Rebuild affected requesters only if the stable Nucleus types or semantics
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
   reads and file compaction.
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
- Re-run relevant domain acceptance tests. Nucleus health proves capability,
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
6. Verify account, strict health and refresh behavior before
   resuming work.

## Diagnosis and recovery

| Observation | Meaning | First action |
| --- | --- | --- |
| `nucleus health` cannot connect | The socket or daemon is unavailable, or the configured path is wrong. | Run `nucleus service status`, inspect the LaunchAgent and Nucleus stderr log, and avoid requester fallback. |
| Health is degraded with an unsupported harness | The configured Codex executable no longer matches the proved adapter. | Restore the proved executable or complete the exact Codex upgrade playbook. |
| `model_auth_unavailable` | The Nucleus-owned credential or account read failed. | Quiesce, perform attended login, verify account and health. |
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

## Readiness and resumption

After a shared change, check matching CLI/daemon versions, service status,
expected harness, authenticated account and affected product readiness. Deployment
has no requirement to create model jobs or synthetic domain records.

Resume only the requester-owned pause or gate set for the operation, preserving
all pre-existing pauses and disabled schedules. Use each product's supported
maintenance interface; do not remove Annals maintenance files manually.

## Where facts and changes belong

Use these placement rules to keep the manual current and small:

- **Operator manual:** current shared topology, authority boundaries,
  compatibility axes, safe ordering, backup, recovery and deployment procedures.
- **Todo:** an unimplemented actionable outcome or researched follow-up.
  “Implement pruning” may be a todo; “Nucleus currently does not prune” is
  current operator truth.
- **CRM:** reusable Markdown profile entries, employment-relationship cases,
  supplied content, case revisions, evidence, steward-run state, and conspicuous nonblocking advisories. It is
  not outreach authority or a scheduler.
- **Cast:** company/job discovery evidence, source coverage and freshness,
  collection configuration, local budgets and the read handoff. Downstream
  selection, application packets and email state remain separate.
- **Platter:** private captured career/posting/template inputs, accepted
  brief and Jackson-bullet stages, rendered resumes, dated editions and send
  outcomes. Production scheduling and installation are not yet implemented.
- **Annals:** retained source material and evidence-grounded conceptual
  knowledge. It may retain released documentation, but it is not the sole
  editable runbook.
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
boundaries, operator action, recovery or deployment procedures change. Prefer
proof commands and versioned authorities over “last verified” dates.

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
- [`nucleus-install`](/Users/joey/rust/cell/nucleus/crates/nucleus-cli/src/bin/nucleus-install.rs):
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

### Cast

- [README](/Users/joey/rust/cell/cast/README.md)
- [Collection contract](/Users/joey/rust/cell/cast/chancery/manuals/discovery-collect.md)
- [Read handoff](/Users/joey/rust/cell/cast/chancery/manuals/discovery-explore.md)
- [Installation and recovery](/Users/joey/rust/cell/cast/chancery/manuals/install-operate.md)

### Platter

Coordinated release-history cleanup recognizes Platter's installation and its
PID-aware file lock. Private packet state and database backups are retained.

- [README](/Users/joey/rust/cell/platter/README.md)
- [Preparation, preview, send and recovery contract](/Users/joey/rust/cell/platter/chancery/manuals/packet-prepare.md)
- [Installation and maintenance contract](/Users/joey/rust/cell/platter/chancery/manuals/install-operate.md)

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

### Chancery

- [Documentation index](/Users/joey/rust/cell/chancery/docs/README.md)
- [Architecture](/Users/joey/rust/cell/chancery/docs/architecture.md)
- [CLI contract](/Users/joey/rust/cell/chancery/docs/cli.md)
- [Provider manifest](/Users/joey/rust/cell/chancery/docs/manifest.md)
- [User-owned installation](/Users/joey/rust/cell/chancery/docs/system-installation.md)

### Shared product installation

The remaining product installers now use shared `cell-install-v2` file
transactions and dedicated Rust `PRODUCT-install` executables. The release
builder prepares all selected products and their declared maintenance closure
in one Cargo invocation before holds. Affected-only products receive a sealed
installer for inspection and recovery but are not upgraded. Requester holds
and drains precede the Nucleus hold so existing continuation work can finish.
Products retain authority over configuration, database backup/recovery,
authentication and schedule/service control; database compatibility must be
proved before public commands are restored. Nucleus's guarded service installer
continues to own its public CLI/daemon copies and forward-only schema boundary.
Cleanup uses only prepared candidate verifiers and retains unverified product
history and selected schedule pins, including disabled bindings. See
[Cell deployment](../../deployment/README.md) for the executable protocol and
interrupted-operation boundaries.

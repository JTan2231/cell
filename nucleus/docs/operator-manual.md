# Nucleus ecosystem operator manual

This manual is the starting point for operating Nucleus, changing a boundary
shared by Nucleus and its requesters, or adding a new requester. It describes
current operational truth and safe change ordering. It is not a backlog,
changelog, protocol specification, or substitute for a requester's domain
documentation.

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

| System | Use it when | It owns | Do not use it for |
| --- | --- | --- | --- |
| Todo | An actionable concern or follow-up should be researched and retained for later. | The todo's immutable source provenance and researched content, open/done state, and append-only working notes. | Work requested for immediate completion, general knowledge, or shared runtime policy. |
| Annals | Immutable source material should be retained or reconciled with an evidence-grounded conceptual corpus, or that corpus should be searched or explored. | Retained works, concepts, evidence, reconciliations, revisions, source deliveries, inbox policy, and domain recovery. | An action backlog, casual notes or preferences, agent-process supervision, or account telemetry. |
| Nucleus | A local application needs constrained agent execution, or shared execution, authentication, compatibility, job history, deployment, or requester integration must change. | Admission, the portable invocation contract, harness validation and supervision, credential serialization, cancellation, raw protocol history, and the durable dynamic-tool mailbox. | Domain success, project registration, workflow graphs, or requester retry policy. |
| Annals Usage | Annals-attributed model consumption, account allowance, login, or the Annals-to-Nucleus execution path must be inspected. | Its reporting projection and Annals-facing budget and diagnostic commands. | Nucleus runtime authority, Codex credential storage, Annals corpus success, or general job orchestration. |
| Codex | Nucleus needs an inspected harness and account protocol implementation. | Its executable and app-server behavior. | Requester domain policy or a second credential authority for Nucleus jobs. |

Typical routing examples:

- “Leave this concern for later” is Todo work.
- “Incorporate this report into what we know” is Annals work.
- “What does the corpus say about predicate locking?” is an Annals query.
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
```

`nucleus health` is strict. It prints the readiness document but exits nonzero
unless the daemon is compatible, authenticated, and accepting jobs. Its output
identifies the daemon and harness versions, executable, protocol versions,
adapter capabilities, and authentication readiness.

A requester's compiled Nucleus client source, the installed Nucleus patch
release, the public protocol version, the store schema, and the exact supported
Codex version are separate compatibility axes. A requester built from an older
compatible client revision can use a newer daemon when they share the same
public protocol. Do not require lockstep product releases without a contract
reason.

## Topology and authority

```text
Todo new -----\
               \
Annals ----------> Nucleus ----------> Codex app-server ----------> account
   |                  |                        |
   |                  |                        `-- isolated job process
   |                  |
   |                  |-- Nucleus SQLite job and raw protocol history
   |                  |-- Nucleus-owned Codex authentication
   |                  `-- durable requester-tool mailbox
   |
   `-- Annals corpus, inbox, reconciliation, and recovery authority

Annals Usage <------ Nucleus logs and account reads
Todo SQLite <------- Todo's validated create_todo operation
```

Only `todo new` follows the Todo-to-Nucleus arrow. Todo's deterministic
lifecycle and email commands read its SQLite database directly. In particular,
the optional daily email path calls Resend without creating a Nucleus job,
using Nucleus authentication, or depending on Nucleus health.

Nucleus is a per-user execution coordinator, not a project registry or workflow
engine. A requester submits one closed, versioned invocation. Nucleus validates
it, starts one harness attempt, retains the protocol, and coordinates dynamic
tool calls. The requester continues to own the work that motivated the job.

Nucleus, Annals, Annals Usage, and Todo share the Cell source repository, Cargo
workspace, and lockfile. That source layout does not merge their release,
installation, state, backup, recovery, or domain-success boundaries.

The following distinctions are operationally important:

1. **Nucleus completion is not domain success.** Todo succeeds when its row is
   durably created. Annals succeeds according to its retained reconciliation
   and delivery state. A model's final prose is diagnostic.
2. **A domain commit can outlive a runtime failure.** If Todo creates its row or
   Annals records its reconciliation and Codex later fails, the durable domain
   result remains authoritative.
3. **A requester restart differs from a daemon restart.** A requester can
   rediscover a durable pending tool call. A Nucleus restart cannot resume a
   Codex process; startup marks unfinished attempts `lost` and their jobs failed.
4. **Registrations are immutable.** Schema IDs and toolset identities preserve
   historical decoding. Change their versions instead of editing old records.
5. **Reporting projections are disposable.** Annals Usage may materialize
   Nucleus records, but it does not become an invocation or domain authority.
6. **One Nucleus job has one attempt.** Nucleus does not automatically retry.
   A requester owns any new domain attempt and its provenance.

### Standard installed paths

```text
~/.local/bin/nucleus
~/.local/libexec/nucleusd
~/Library/LaunchAgents/org.nucleus.daemon.plist

~/Library/Application Support/Nucleus/
  nucleus.sock
  nucleus.db
  nucleus.db-wal
  nucleus.db-shm
  codex-home/

~/Library/Logs/Nucleus/
  nucleusd.stdout.log
  nucleusd.stderr.log
```

The complete Nucleus state directory is sensitive. The database can contain
prompts, tool arguments and results, source content emitted by an agent, and
the exact raw app-server exchange. The Nucleus-owned Codex home contains the
authoritative credential and may contain additional Codex-local state. The
Unix socket has no application-level authentication; local user ownership and
filesystem permissions are the trust boundary. There is no TCP listener in
protocol version 1.

Annals and Todo have their own state, installation, backup, and recovery
boundaries. Do not infer their state from Nucleus or copy their detailed
procedures into this manual. Todo's optional
`~/Library/LaunchAgents/org.todo.daily-email.plist` is a separate user service:
launchd invokes Todo at 09:00 machine-local time, its zsh runner sources
`RESEND_API_KEY` from `~/.zshrc`, and its logs live under
`~/Library/Logs/Todo/`. It is not part of `org.nucleus.daemon` or Nucleus's
credential lease.

## Compatibility model

| Axis | Authority | How to inspect | Change consequence |
| --- | --- | --- | --- |
| Nucleus release | CLI and daemon package versions | `nucleus --version`, `nucleus health` | Candidate CLI and daemon versions must match. A patch need not force requester releases when public semantics are unchanged. |
| Public invocation protocol | `nucleus-core`, HTTP contract, runtime contract | `supportedProtocolVersions` in health and the request types | Additive support can be deployed daemon-first. An incompatible change requires a new protocol version and coordinated requester cutover. |
| Nucleus store schema | `nucleus-store` schema and migration code | SQLite `PRAGMA user_version` and the source constant | A newer schema can make binary-only rollback unsafe. It needs an explicit migration and database rollback plan. |
| Codex harness | `nucleus-codex` adapter and semantic checks | Harness identity in health | The adapter supports an exact inspected Codex release. Update and prove the adapter before replacing the executable. |
| Requester client build | Shared Cargo workspace and requester adapter | Workspace manifests, lockfile, and requester source revision | Rebuild when it needs changed types or behavior. Runtime compatibility still follows the public protocol, not source lockstep. |
| Log schema and toolset | Immutable Nucleus registrations plus requester code | Registration identity and digest | Publish a new schema ID or toolset version. Historical jobs keep the old decoder and definition. |
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

Inspect one job and its ordered schema-bound records with:

```sh
nucleus jobs show JOB_ID
nucleus jobs logs JOB_ID
nucleus jobs logs --follow JOB_ID
```

Scope a search to one domain run when the requester identity is known:

```sh
nucleus jobs list --requester PROGRAM --requester-id REQUESTER_ID
```

`nucleus account --wait 0` is a nonblocking credential-lease probe. An
`authentication_busy` result means another job, account read, refresh, or login
currently owns the lease; it does not by itself mean the credential is invalid.

### Quiesce before work that cannot tolerate a lost attempt

Nucleus has no global drain mode. Quiescence is established at its requesters:

1. Do not start a synchronous Todo creation or another manual requester job.
2. Pause Annals and wait for its active delivery to settle:

   ```sh
   annals inbox pause
   annals inbox status
   ```

3. Inspect Nucleus `accepted`, `running`, and `waiting-on-requester` jobs.
4. Wait for them to become terminal. Cancel a job only when abandoning that
   exact runtime attempt is intended:

   ```sh
   nucleus jobs cancel JOB_ID
   ```

5. Perform the service, storage, or harness operation.
6. Verify Nucleus and requester canaries before resuming Annals.

Stopping or replacing `nucleusd` while a job is active makes that attempt
`lost`. The requester—not Nucleus—decides whether a new domain attempt is safe.
The Todo daily-email LaunchAgent is not a Nucleus requester and does not need
to be paused to establish Nucleus quiescence.

### Authentication recovery

Nucleus owns one authoritative Codex home under its private state directory.
Jobs, account reads, refreshes, and attended login share one exclusive
credential lease. Annals and Todo do not read or refresh the credential.

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

Nucleus version 1 has no automatic retention, pruning, backup, or restore
command. Use an explicit, private backup destination and preserve its access
controls.

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
   recovery objective requires them. Nucleus state does not replace Annals or
   Todo backups.
7. Bootstrap the unchanged service and verify health:

   ```sh
   launchctl bootstrap "gui/$(id -u)" \
     "$HOME/Library/LaunchAgents/org.nucleus.daemon.plist"
   nucleus health
   ```

Restoration is an attended operation. Quiesce and stop the service, preserve
the current state separately, restore a database only to a binary known to
support its schema, then validate health and historical job/log reads before
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

Apply the host's normal private-log rotation policy to Nucleus stdout and
stderr. Do not delete rows from `nucleus.db`, edit immutable registrations, or
remove individual Codex-home files as ad hoc retention. A supported pruning
policy must first define which audit and coordination relationships remain
valid, implement that policy in Nucleus, and include migration and recovery
tests. Until then, monitor, back up, and retain the state.

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

Use a launch context only when the job must observe the requester's caller
environment. Register the complete snapshot immediately before submission. The
context is requester-bound, memory-only, single-use, expires after 120 seconds,
and is not a durable configuration mechanism.

Minimize authority. Todo deliberately needs read-only local and web research;
Annals deliberately receives neither builtin shell nor web access and operates
through its bounded tools. A new requester should justify its own combination
instead of copying either profile.

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

### 6. Implement the lifecycle

A normal requester flow is:

1. Check Nucleus readiness and, when domain admission depends on it, account
   readiness.
2. Register required schemas and toolsets idempotently.
3. Register a launch context if needed.
4. Submit the exact request.
5. Long-poll the durable tool-call mailbox while the job is nonterminal.
6. Validate each call, perform the domain operation through the requester's
   backend, durably bind or cache its exact result, and post that result.
7. Read the terminal job and structured attempt output.
8. Use the requester's durable state to decide domain success.
9. Read Nucleus logs for runtime diagnostics and audit, not as a replacement
   for the domain result.

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
- timeout or cancellation;
- daemon restart and a `lost` attempt;
- Nucleus completion without the required domain result; and
- domain success despite later runtime failure.

A genuinely new attempt uses a new job ID. Preserve the same domain-run
correlation when appropriate. Nucleus must not decide whether that attempt is
allowed.

### 8. Add observability, security, and acceptance proofs

At minimum, test:

- strict health and required capabilities;
- successful admission and domain completion;
- identical and conflicting duplicate job submissions;
- identical and conflicting duplicate tool results;
- requester restart while waiting on a durable tool call;
- daemon loss during an active attempt;
- cancellation and timeout;
- authentication busy and unavailable behavior;
- unsupported model, harness, working directory, or permission combinations;
- durable domain success followed by runtime failure; and
- absence of a direct-runner fallback.

Treat both the requester's state and Nucleus raw logs as sensitive according to
the content they can retain. Add a requester canary, backup coverage, release
ordering, rollback boundary, and operator documentation before production use.

### 9. Add its capability relationship

If the requester exposes a distinct user-facing durable outcome, add one short
capability card to this manual and the global routing instructions. State its
positive triggers, explicit non-triggers, authority, and Nucleus relationship.
Keep detailed CLI and domain procedures in its own product tree.

## Route changes by their authority

| Change | Primary authority | Cross-system obligations |
| --- | --- | --- |
| Todo creation, lifecycle, provenance, database, email delivery, or deployment | Todo | Preserve its Nucleus adapter contract when affected; the direct Resend path does not become a Nucleus job, and Nucleus does not gain Todo fields. |
| Annals works, concepts, evidence, reconciliation, inbox, retry, or corpus migration | Annals | Preserve job correlation and adapter behavior when affected; Nucleus does not gain Annals workflow state. |
| Annals usage attribution, budget display, or diagnostic projection | Annals Usage | Read Nucleus records through the supported interfaces; do not become runtime or corpus authority. |
| New portable invocation meaning or HTTP behavior | Nucleus core/client/daemon | Version the public contract, update examples/tests/docs, then update affected requesters in compatible order. |
| Codex executable or app-server semantics | Nucleus Codex adapter | Prove the exact version, deploy Nucleus, then run generic and requester canaries. |
| Nucleus database schema or retention | Nucleus store | Quiesce, back up, migrate and validate, and define database-aware rollback before deployment. |
| Requester tool arguments, result, or definition | Requester plus immutable Nucleus registration | Publish a new schema/toolset version and keep historical decoding. |
| Requester prompt, model, timeout, or permission profile | Requester | Use new job IDs for new attempts, verify health capabilities, and rerun domain acceptance tests. |
| Credential or credential-lease behavior | Nucleus | Quiesce all credential consumers, preserve forward-only authentication, and canary every requester. |
| Nucleus service layout or installer | Nucleus CLI/packaging | Preserve state/log ownership, rollback, launchd behavior, and requester configuration. |

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

   Its 60-second budget covers only the six Nucleus packages and Nucleus's
   shell and packaging checks. A root aggregate CI run does not replace or
   shorten that per-product budget.

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

6. The installer stages files, replaces the LaunchAgent, and checks health. A
   failed cutover restores its captured binaries and service configuration.
   Authentication is deliberately excluded from rollback.
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
6. Run a fresh Nucleus job, a deliberate Todo creation when affected, and an
   Annals reconciliation canary when affected.
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

The current store initializes schema version 1 and rejects a newer schema, but
it has no ordered upgrade path from version 1 to a later version. Before schema
version 2 or any later change:

1. Implement explicit, incremental migrations from every supported prior
   version.
2. Add a real old-schema fixture containing representative jobs, attempts,
   schemas, logs, and pending or answered tool calls.
3. Prove migration is transactional and idempotent where repetition is allowed.
4. Quiesce requesters, stop Nucleus, and take a consistent pre-migration backup.
5. Define whether the old binary can read the migrated database. If not,
   rollback requires both the old binaries and the pre-migration database;
   installer binary rollback alone is unsafe.
6. Migrate and validate historical reads, digests, lifecycle projections,
   foreign keys, and new behavior before requester canaries.
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
3. Preserve private directory and file modes and the exclusive lease across
   login, account reads, job copy-in, refresh, and copy-back.
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
| `authentication_busy` | Another credential user owns the exclusive lease. | Wait or use the requester's documented bounded wait; do not replace credentials. |
| Job is `waiting_on_requester` | A durable dynamic tool call has not received its requester-owned result. | Inspect pending calls and the requester process/domain state. Restart the requester if it supports mailbox recovery; do not invent a result manually. |
| Attempt is `lost` | Nucleus restarted while the harness attempt was unfinished. | Inspect domain state first. Let the requester decide whether and how to create a new attempt. |
| Nucleus job failed after a domain commit | Runtime completion failed after the requester established success. | Preserve the domain result, correlate the Nucleus failure for diagnostics, and do not repeat the mutation. |
| Nucleus job completed without the required domain record | The model turn completed but the requester did not establish domain success. | Follow requester failure policy; Nucleus completion alone is insufficient. |
| Installation reports rollback | Candidate health or cutover failed and captured program/service artifacts were restored. | Verify the restored service and forward-only authentication, inspect logs, and do not assume a database migration was rolled back. |
| State or logs grow continuously | Version 1 has no automatic pruning or log rotation. | Measure both paths, apply private host log rotation, and plan supported retention rather than deleting rows. |

For Annals failures, use its status, pause, interruption, and bounded retry
procedures. Never move failed envelopes back into the queue or edit their
receipts. For Todo, inspect the Todo database/result first; a committed creation
wins over later runtime failure. Detailed recovery policy remains in each
requester.

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
   ordered logs.
5. **Resumption:** clear only the requester-owned pause or gate that was set for
   the operation. Do not remove Annals maintenance files manually.

An Annals integration canary creates a real examination and reconciliation
record even without `--apply`; choose the work deliberately and inspect the
result. A Todo canary creates a real todo; choose a real source and direction or
use an isolated Todo database. Do not leave unexplained canary domain records.
Sending or previewing Todo's email digest does not exercise Nucleus and is not
a requester canary.

## Where facts and changes belong

Use these placement rules to keep the manual current and small:

- **Operator manual:** current shared topology, authority boundaries,
  compatibility axes, safe ordering, backup, recovery, and canary obligations.
- **Todo:** an unimplemented actionable outcome or researched follow-up.
  “Implement pruning” may be a todo; “Nucleus currently does not prune” is
  current operator truth.
- **Annals:** retained source material and evidence-grounded conceptual
  knowledge. It may retain released documentation, but it is not the sole
  editable runbook.
- **Component documentation:** exact Nucleus protocol, Todo creation behavior,
  Annals corpus and inbox behavior, or Annals Usage accounting.
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
them into Nucleus's 60-second product CI.

## Reference map

The absolute checkout paths below are intentional: this file is also embedded
in `nucleus manual`, so its links must work when the command is run from any
directory.

### Nucleus

- [README](/Users/joey/rust/cell/nucleus/README.md): build, install, readiness,
  storage warning, and smoke entry points.
- [Runtime contract](/Users/joey/rust/cell/nucleus/docs/runtime-contract.md): exact
  request, harness, mailbox, raw log, HTTP, authentication, and security
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

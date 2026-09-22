# Integrate a Nucleus requester

A Nucleus requester integrates an application with shared execution. It is not
a registered project. The application owns its durable result; Nucleus owns
execution. Before each integration or shared contract change, read the installed
manual for that version:

```sh
/Users/joey/.local/bin/nucleus manual
```

## Define the domain boundary first

Before designing an invocation, identify:

- the exact durable condition that means the application operation succeeded;
- the database, filesystem, or service authoritative for that condition;
- the tools allowed to mutate that authority;
- idempotency behavior for duplicate delivery;
- who decides whether another attempt is safe; and
- the proof a person can inspect to distinguish domain success from runtime
  success.

If Nucleus would need to understand application-specific rows or workflow
states to answer those questions, the boundary is wrong.

## Integration contract

Rust requesters in Cell use the workspace `nucleus-core` and `nucleus-client`
sources. Another language may implement the documented HTTP protocol over the
per-user Unix socket. Do not shell out to the human CLI when a typed client or
HTTP surface is available.

Choose a stable lowercase requester program, a domain-run requester ID, and a
unique Nucleus job ID. Persist correlation in both directions. An ambiguous
submission may repeat only the byte-equivalent request under the same job ID;
different content under the same ID is a conflict.

Every invocation policy is explicit: harness, model, reasoning effort,
absolute working directory, workspace access, local execution, web search,
timeout, launch context, and optional dynamic toolset. Require strict health
and the exact protocol, adapter, and execution-capacity capabilities needed by
the requester. Health exposes Nucleus's global maximum of eight active attempts
as `maxActiveJobs` and the live `activeJobs` and `availableSlots` counts.

Invocation policy version two adds `workspaceAccess=unrestricted` with
unsandboxed current-user filesystem, process, local socket, and network access.
Require `workspace-unrestricted` in the harness capabilities and enable local
execution when commands are needed. `approvalPolicy=never` remains fixed.
Built-in web search is a separate tool choice. Version-one policies retain
their existing access modes. The job and HTTP protocol stay version one.
Deploy accepting daemon support first, and preserve compatible decoding for
retained version-two requests.

Admission does not require a free execution slot. A newly admitted job remains
`accepted` with its sole attempt `pending` until a slot is available. The
invocation timeout begins only when that slot is acquired. An attempt in
`waiting_on_requester` still owns its slot because the supervised Codex process
remains live. Nucleus schedules capacity only; it does not own the requester's
work-packet graph, priorities, success rule, or retry policy.

Before submitting concurrent `read-write` or `unrestricted` jobs, assign
disjoint working directories or worktrees, or serialize them in the requester. Nucleus does not
compare paths or coordinate filesystem and external-mutation conflicts.

Decoder schemas and toolset registrations are immutable by identity and
digest. When their meaning changes incompatibly, publish a new version and
retain the decoder for historical jobs. Never rewrite an old registration.

## Runtime lifecycle

The normal lifecycle is:

1. Verify strict Nucleus readiness and any domain admission prerequisites.
2. Register immutable schemas and toolsets idempotently.
3. Persist correlation and the exact typed request before submission.
4. Submit the request.
5. Tolerate an accepted/pending interval, then long-poll the durable
   requester-tool mailbox while the job is nonterminal.
6. Validate each call, commit the requester-owned mutation idempotently, bind
   the exact result durably, and post it.
7. Read terminal job and structured output state.
8. Decide success from requester-owned state.
9. Use Nucleus output atoms for protocol diagnosis or live reporting, never as
   a replacement domain record.

There is no hidden direct-Codex fallback. A requester restart may rediscover a
pending durable call. A Nucleus restart cannot resume the app-server process;
it marks the attempt lost. Only the requester can authorize a new attempt.

Nucleus's managed authentication remains one private authority even while jobs
run concurrently. Account reads may overlap active jobs. Nucleus serializes
canonical credential refresh, and attended login is excluded until all active
job and account sessions have ended; requesters never read, refresh, or copy
the canonical credential themselves.

## Codex weekly quota admission

Treat quota deferral as an expected admission pause. A healthy daemon can report
`status=ok`, `acceptingJobs=false`, and a blocked `quota`. The default gate pauses
main-Codex work at 10% remaining or less and reopens above 15% after a fresh
observation. If no valid weekly observation remains, admission pauses as `unknown`.
API-key authentication has no subscription weekly gate.

Use Nucleus's cached quota condition; do not read its credential or duplicate its
allowance checks. `GET /v1/quota` returns that condition without model work.
Quota recovery clears no deployment hold, operator pause, or Clockwork failure
halt.

A rejected new submission returns HTTP 429 `quota_deferred`, with the quota
snapshot in response `details`. It creates no job or attempt. The Rust client
returns `ClientError::QuotaDeferred`. Preserve pending domain work and its exact
request identity. An exact replay of an admitted request remains available.

Accepted jobs recheck quota before execution. While paused, they retain their
pending attempt without a slot or a running execution timeout. Job reads attach
the condition to pending main-Codex jobs. `get_job_for_work` returns a typed
deferral; raw `get_job`, mailbox reads, cancellation, status, and authentication
remain available. Started attempts continue.

Return success with an explicit quota outcome for a deferred scheduled
activation. Do not report an abend. Keep existing deadlines and selection rules;
do not replay expired work automatically. EMT owns the shared condition notice
and sends it through Email without a Nucleus job.

A structured Codex `usageLimitExceeded` ends the attempt with `quota_exhausted`
and pauses new admission. Inspect domain effects before authorizing a retry;
a committed result remains authoritative. A Nucleus restart marks unfinished
attempts lost, including pending attempts. A quota pause does not authorize replay
after restart.

Upgrade all requester clients before enabling the gate on Nucleus. Clients must
tolerate a daemon without optional quota health fields. Use coordinated
maintenance for cutover.

## Required checks

Test these behaviors:

- Strict health, capacity reporting, eight active attempts, and accepted/pending
  waiting for later work.
- Admission, domain completion, identical and conflicting job submissions, and
  identical and conflicting tool results.
- Requester restart with a pending call, daemon loss, queued and active
  cancellation, and domain success followed by runtime failure.
- Timeout after slot acquisition and slot retention while waiting on a requester.
- Concurrent account reads, serialized refresh, and login exclusion.
- Unsupported invocation combinations and the absence of a hidden execution path.
- Requester ownership of work packets, write conflicts, and retries.

Add requester observability, private-state handling, backup coverage, release
ordering, rollback boundaries, operator documentation, and service readiness
checks that do not submit model jobs or create synthetic domain records.

## Authority and authorization

This operation does not authorize domain changes beyond the requested
integration, a production cutover, release publication, or a retry of failed
application work. Shared protocol, store, authentication, service, and
compatibility changes follow the guarded Nucleus playbooks and may require
coordinated requester work.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

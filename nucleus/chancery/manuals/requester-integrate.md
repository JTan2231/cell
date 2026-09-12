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

Admission does not require a free execution slot. A newly admitted job remains
`accepted` with its sole attempt `pending` until a slot is available. The
invocation timeout begins only when that slot is acquired. An attempt in
`waiting_on_requester` still owns its slot because the supervised Codex process
remains live. Nucleus schedules capacity only; it does not own the requester's
work-packet graph, priorities, success rule, or retry policy.

Before submitting concurrent `read-write` jobs, assign disjoint working
directories or worktrees, or serialize them in the requester. Nucleus does not
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

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.

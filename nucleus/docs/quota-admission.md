# Quota admission

This reference defines the main-Codex weekly admission gate. API-key
authentication has no subscription weekly gate.

## Observation and policy

`nucleus quota` and `GET /v1/quota` read the cached admission condition without
starting a model turn. Every 60 seconds, Nucleus reads Codex App Server
`account/rateLimits/read` through its own credential authority. It selects
`rateLimitsByLimitId.codex` and the single primary or secondary window with
`windowDurationMins=10080`. Remaining percent is `100 - usedPercent`. Nucleus
uses the legacy `rateLimits.limitId=codex` bucket only when the map is absent.
It never substitutes the Spark bucket. Null, absent, ambiguous, expired, or
malformed weekly data is unknown, not zero or unlimited.

The default policy pauses new work at 10% remaining or less. Admission reopens
only after a fresh observation exceeds 15%. An observation is valid for at most
120 seconds and never past its reported reset. A failed read can use a valid
cached observation; otherwise admission pauses as `unknown`. The reset time
alone does not reopen admission.

Quota applies to the whole account. Other CLI and desktop sessions can exhaust
it between samples. The threshold does not reserve tokens or guarantee that
active work finishes.

## Configuration and retained state

`quota-policy.json`, beside `nucleus.db`, configures the gate at daemon startup:

```json
{"enabled":true,"pauseAtRemainingPercent":10,"resumeAboveRemainingPercent":15}
```

Set `0 <= pause < resume < 100`. Keep this file and `quota-state.json` as private
regular files with mode 0600. Invalid files prevent startup. Include both files
in private Nucleus backups.

Nucleus writes state atomically. State contains the policy, account-identity
digest, `limitId`, `state`, remaining percentage, observation and reset times,
and one condition ID for a continuous pause. Times are Unix seconds. Missing
numeric values remain null. State and condition identity survive restart.
Account changes require a new observation. Do not edit state to simulate recovery.

## Admission and execution

A rejected new submission returns HTTP 429 with code `quota_deferred` and the
quota snapshot in response `details`. It creates no job or attempt. The Rust
client returns `ClientError::QuotaDeferred`. An exact replay of an admitted
request remains available.

Accepted jobs recheck quota before execution. While paused, they retain their
pending attempt without an execution slot or a running execution timeout.
Job reads attach the quota condition to pending main-Codex jobs.
`get_job_for_work` returns a typed deferral for those jobs. Raw `get_job`,
mailbox reads, cancellation, status, and authentication remain available.

Started attempts continue. A structured Codex `usageLimitExceeded` becomes
terminal reason `quota_exhausted`. Nucleus pauses further admission immediately.

## Requester recovery

Requesters preserve pending work and immutable request identity on deferral.
Scheduled activations return success with an explicit quota outcome and do not
report an abend. Existing deadlines and daily-report selection still apply.
Expired work is not replayed automatically. Past Paperboy periods require
selection of their retained brief. Todo keeps its bounded wait after acceptance.

Quota exhaustion after a start remains a retained failed attempt. Inspect domain
effects before authorizing a retry. A committed domain result remains
authoritative. Nucleus restart marks unfinished attempts lost, including pending
attempts. A quota pause does not authorize replay after restart.

A healthy daemon can report `status=ok`, `acceptingJobs=false`, and a blocked
`quota`. Quota recovery clears only its own condition. Deployment holds, operator
pauses, and Clockwork failure halts remain independent.

## Notifications and rollout

EMT's worker freezes one deterministic notice per condition ID. It sends the
notice directly through Email without a Nucleus job. Unknown quota has distinct
wording. Notice identity and transport progress survive restart in EMT's private
`quota-notifications/` directory. Include these files in EMT backups.

EMT permits at most two transport invocations with the same key and payload,
five minutes apart and within 23 hours. An unresolved send remains uncertain
and requires inspection. It does not create a replacement message or model job.
This prevents quota failure notices from each service without suppressing
unrelated incidents.

Upgrade all requester clients before enabling the gate. Use coordinated
maintenance for cutover. Clients tolerate a daemon without optional quota health
fields; EMT also tolerates the old quota endpoint's 404. This procedure does not
authorize rollout, a quota reset, or clearance of existing service halts.

Deployment readiness accepts a healthy runtime under a reported quota pause.
It still requires the exact harness, authentication, supported protocol, and
execution state. A deployment hold must belong solely to that run and be
drained. Successful installation does not clear quota admission; ordinary
`nucleus health` remains strict about accepting jobs.

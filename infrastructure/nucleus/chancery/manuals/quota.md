# Quota admission

Use this feature to inspect or configure the cached Codex weekly admission
condition and to interpret quota deferral or exhaustion. Quota controls new
runtime work; it does not own requester deadlines, retry decisions, or domain
success. `nucleus quota` and `GET /v1/quota` inspect the retained condition without
starting a model turn.

The supported interfaces use the current-user CLI or version-one HTTP over
Nucleus's filesystem-protected Unix socket. There is no TCP endpoint or
application-level authentication. Reading a Chancery entry grants no access or
authority to submit, cancel, mutate requester data, or control the service.

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
with private Nucleus state.

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
Expired work is not replayed automatically. Each requester owns its own
deadlines, selection rules, and bounded waits.

Quota exhaustion after a start remains a retained failed attempt. Inspect domain
effects before authorizing a retry. A committed domain result remains
authoritative. Nucleus restart marks unfinished attempts lost, including pending
attempts. A quota pause does not authorize replay after restart.

A healthy daemon can report `status=ok`, `acceptingJobs=false`, and a blocked
`quota`. Quota recovery clears only its own condition. Deployment holds, operator
pauses, and Clockwork failure halts remain independent.

## Notifications and rollout

EMT owns the shared notice for each quota condition. It sends directly through
Email without a Nucleus model job. Discover and read the applicable installed
EMT and Email contracts through Chancery before operating their notices. A
Nucleus quota observation does not establish notice delivery or authorize
another send.

Upgrade all requester clients before enabling the gate. Use coordinated
maintenance for cutover. Clients tolerate a daemon without optional quota health
fields; EMT also tolerates the old quota endpoint's 404. This procedure does not
authorize rollout, a quota reset, or clearance of existing service halts.

Deployment readiness accepts a healthy runtime under a reported quota pause.
It still requires the exact harness, authentication, supported protocol, and
execution state. A deployment hold must belong solely to that run and be
drained. Successful installation does not clear quota admission; ordinary
`nucleus health` remains strict about accepting jobs.

## Limits and private state

Keep the policy and state private and back them up with Nucleus state. Changing
the configuration takes effect at daemon startup; do not edit the retained
condition to simulate recovery. This feature does not authorize a service
restart, a quota reset, rollout, or clearance of any independent halt.

Codex owns the external allowance and other sessions consume the same account.
The sample cadence and validity bounds do not promise reserved tokens,
completion of admitted work, billing equivalence, or a service-availability
objective. Documentation installation does not prove live quota or readiness.

## Related contracts

- Read `chancery show nucleus.jobs`.
- Read `chancery show nucleus.authentication`.
- Read `chancery show nucleus.service`.
- Read `chancery show nucleus.requester.integrate`.

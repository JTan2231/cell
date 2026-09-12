# Quota admission


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

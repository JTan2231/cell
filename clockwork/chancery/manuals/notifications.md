# Pause alerts and delegated notification ownership

Use this feature to understand eligibility, send one due halt alert, recover
uncertain transport, or configure initial notification handoff to EMT.
Clockwork owns check progress, the basic metadata payload, and routing and
claim metadata. Iatreion and product probes own observed operational facts.
Email owns credentials and provider transport. EMT owns diagnosis,
correspondence, delegated transport, and its uncertainty recovery.

## Interfaces

```text
clockwork [--json] notification send
clockwork [--json] notification retry INCIDENT_ID
clockwork [--json] notification policy [--failure-threshold N] [--interval-seconds SECONDS] [--cell-root ABS]
clockwork [--json] notification check
clockwork [--json] notification emt --receiving-domain DOMAIN
clockwork [--json] notification emt --disable
clockwork [--json] notification show INCIDENT_ID
clockwork [--json] notification claim INCIDENT_ID --delivery-id UUID
```

Successful commands return an `ok:true` / `data` JSON envelope. `--json`
selects compact output and coded `ok:false` / `error` failures on stderr with
exit one; otherwise failures are human-readable.

## Basic eligibility and transport

Each halt carries standing authority for one plain-text notification to Email's
fixed personal recipient. The subject and body identify the binding, incident,
failure code, occurrence, activation, halt time, and inspection/continuation
commands. These metadata are disclosed to Email, Resend, and Gmail. Clockwork
retains no product output, email response body, or credential. Email loads its
own credential and owns bounded HTTP transport; exit zero establishes provider
acceptance, not inbox delivery.

New schema-two activation failures start pending episodes without an immediate
halt or incident. Later activations remain admissible during this delay. The
shared policy requires five consecutive failed read-only service checks by
default, at least 60 seconds apart. At the threshold, Clockwork establishes the
halt and alert eligibility together and retains pending mail with the incident.

Clockwork uses the installed Iatreion report for the configured Cell checkout.
Historical domain outcomes do not count as service-check failures. Unknown
health counts as failed with an explicit unknown condition. A healthy worker
observation, a later successful activation without an abend, or explicit inactive
intent, including operator pause, clears a pending episode without clearing its
failure evidence. No check runs or retries product work.

Existing halts remain closed and keep their current notification-check progress.
Healthy or inactive checks suppress an unalerted notification episode; explicit
incident resumption also suppresses it. Existing delivery attempts and claims
retain their recovery rules. An explicit `binding halt` still closes admission
immediately; its notification retains the service-check gate.

Configure the notification policy and inspect progress:

```sh
clockwork notification policy --failure-threshold 5 --interval-seconds 60 --cell-root /absolute/cell
clockwork notification check
clockwork notification show INCIDENT_ID
```

`notification check` advances due observations without sending mail or running
product work. It can establish a new halt at the shared threshold. Its result
retains policy and incident checks and adds a `failures` map keyed by binding
for pending event and check progress. `notification show` selects an existing
incident and returns `health_check.threshold`, `count`, `last_checked_at`,
`condition` and `eligible_at`. Preexisting incidents can still have progress
below their notification threshold. New pending episodes have no incident ID
to pass to show. A continuous eligible episode creates no repeated alerts. The
default checkout is `$HOME/rust/cell`; Iatreion must be installed at
`$HOME/.local/bin/iatreion`. Policy changes do not resume schedules or retry work.

Existing broker visits and the EMT worker advance due checks. Every scheduled
or manual broker visit attempts at most one eligible due notification before
the product gate, and one after the activation outcome. Attempts use a
private transport lock, a fixed payload and idempotency key, a 120-second
process bound, and at least five minutes between attempts for one incident.
The halt remains closed when transport or notification bookkeeping fails.
Other timers can deliver an alert for a disabled or halted product. There is
no extra daemon or timer; if no broker is invoked, pending mail waits.

Run `clockwork notification send` to attempt one due notification independently
of product scheduling. Read its incident to distinguish `pending`, `accepted`,
and `uncertain`. An interrupted or failed transport may already have been
accepted. After 23 hours from the first invocation, or a backwards clock jump
before that invocation, Clockwork makes no automatic further attempt and
retains `uncertain`, leaving margin before Resend's 24-hour deduplication limit.
Inspect provider acceptance before issuing
`clockwork notification retry INCIDENT_ID`. That command explicitly approves
possible duplication, creates a new idempotency generation, and attempts the
same retained incident payload. It never clears the scheduling halt.

## EMT routing and ownership

An operator can configure EMT preference for new incidents:

~~~sh
clockwork notification emt --receiving-domain RECEIVING_DOMAIN
clockwork notification emt --disable
clockwork notification show INCIDENT_ID
clockwork notification claim INCIDENT_ID --delivery-id UUID
~~~

The receiving domain is an Email account routing destination. Configuration
does not initialize, schedule or run EMT. It grants EMT the opportunity to own
the initial notification for newly routed incidents. EMT's own emt/worker
incidents always use the basic notification path.

Clockwork stores version-one metadata in notification-routing.json under its
private state root. It contains the configured domain, activation time, saved
incident reply routes, grace deadlines and optional EMT delivery UUIDs. It
contains no diagnostic text, received mail or credential. The schema-two
database remains unchanged. The separate private `notification-checks.json`
sidecar retains policy and incident notification-check progress. The schema-one
`failure-checks.json` sidecar retains the immutable abend ledger cursor and
pending per-key episodes and check progress. Back up and recover all three
sidecars with the database; a database-only backup does not preserve pending
failures, delegated ownership or alert progress.

The first notification inspection or sender visit snapshots an eligible
incident's Reply-To. Its basic-send grace deadline is 120 seconds after the
service-check threshold is reached. Notification show can therefore create that
metadata under the notification lock. It
returns the basic subject/body, saved Reply-To and optional delivery ID.
The basic rendering remains fixed; existing attempted notifications do not
acquire new headers.

EMT persists its exact outgoing email before claiming ownership. Claim uses
the same lock as basic transport. It requires health-check eligibility and an
unattempted pending notification with an EMT route. It is idempotent for the same
delivery UUID.
An existing different claim or started basic send refuses the claim. A basic
send begins only after the grace deadline and only when no EMT claim exists.

Claims never expire. After a claim, EMT owns delivery and uncertainty recovery.
Clockwork's incident notification_status and attempt counts still describe
Clockwork transport only; read EMT for delegated acceptance. Configuration
changes preserve saved routes and claims. No notification operation clears
the scheduling halt.

If basic submission starts first, EMT can send its report as a follow-up. The
basic message's incident-specific reply address also routes to EMT. An agent
failure or unavailable Nucleus does not prevent an unclaimed basic alert.
There is no independent notification timer or final-delivery guarantee.

Before enabling EMT, refresh all active generated plists to a Clockwork broker
that understands this handoff. An older pinned broker ignores the routing
sidecar. Refresh every enabled pinned broker for the new failure-check contract.
Do not run or restore an older broker while `failure-checks.json` exists or
with delegated ownership present. Preserve database, all sidecars and EMT
exchange state together during recovery;
do not erase claims to force another send after uncertain acceptance.

EMT's five-minute diagnosis deadline starts when the incident becomes eligible.
Clockwork runs no agent and retains no diagnosis or correspondence body.
Clockwork's own `notification send` count means attempted invocations, not
accepted deliveries. Notification attempt times are whole Unix seconds.
Email idempotency identity combines incident, explicit retry generation, and
fixed rendering version. EMT delivery ownership uses the claimed UUID.

## Retention, compatibility, and limits

The incident freezes its Email wrapper path and rendering-version-one inputs.
The database stores status, first/last attempts, total invocations and explicit
retry generation; the basic body is derived from metadata. It stores no email
body, provider response body, credential, or product output. The routing
sidecar uses private atomic replacement and directory sync. Existing routes,
claims, and attempted headers remain fixed across policy changes.

The schema-two database, schema-one failure-check sidecar, version-one routing
sidecar, notification-check metadata, provider release, Email contract, and EMT
contract are separate compatibility axes.
Refresh pinned brokers before relying on check eligibility or delegated
ownership. Older brokers ignore that metadata. Back up and restore database,
all three sidecars, and EMT exchange evidence coherently. Losing a claim can
duplicate
an already accepted message. Do not erase sidecars to force a send.

No independent timer, final delivery guarantee, service-check freshness
objective, alert-latency bound, automatic pruning, or deprecation window is
promised. Notification authority is limited to the retained personal halt
alert; it does not authorize unrelated mail, schedule continuation, or product
retry. Scheduled check progress relies on later broker visits or the existing
EMT worker, whose timer delivery remains subject to launchd.

Use `clockwork.schedule.operate` for procedure checkpoints. Read
`clockwork.incidents` for halt meaning, `clockwork.installation` for broker
cutover and backup, `email.message.send` for transport, and
`iatreion.status.inspect` for operational evidence.

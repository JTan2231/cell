# Install and schedule Paperboy

Use this operation to deploy Paperboy, inspect readiness, and control its daily
Clockwork binding. Installation, schedule activation, and report sending are
separate effects. Catalog presence does not authorize any of them.

## Prepare and deploy

Cell deployment uses committed local `main`, compatible candidates, and the
Nucleus requester maintenance closure. Complete the required development checks
before publication or deployment. From the Cell root:

```sh
./deploy.sh plan paperboy
./deploy.sh paperboy
```

Decision reports require the Annals feed's `start` operation. Deploy the updated
Annals and Paperboy together when adding this support:

```sh
./deploy.sh plan annals paperboy
./deploy.sh annals paperboy
```

The shared builder seals production artifacts. The coordinator holds affected
requester admission, drains existing work, and selects the exact program and
provider release. Paperboy owns its private database and recovery. Nucleus owns
execution and credentials; Conversations owns history reads; Email owns submission;
Clockwork owns activation, durable scheduling halts and incident notification.

Deployment initializes an absent schema-one database or backs up an existing
supported database before candidate selection. Unsupported versions stop it.
Held readiness checks create no report or email. Installation succeeds after
exact candidate readiness and release of the run-owned holds.
Direct installer publication and rollback are unavailable; use the coordinator.

## State and inspection

The private database is
`~/Library/Application Support/Paperboy/paperboy.sqlite`.
It retains briefs, exact agent requests and tool replies, accepted summaries,
and email attempts. Records can contain private conversation text.

```sh
paperboy init
paperboy doctor
paperboy doctor --report decisions --annals-config /absolute/decisions.toml
paperboy-install inspect
```

`init` creates empty schema-one state. Doctor reports product readiness; inspect
reports the maintained installation. A program selection does not prove that a
scheduled binding selects that release.

## Maintain admission and back up

```sh
paperboy --json maintenance hold OWNER
paperboy --json maintenance status
paperboy --json maintenance drain
paperboy --json maintenance release OWNER
CELL_DEPLOYMENT_RUN_ID=OWNER paperboy migrate --backup ABSENT_ABSOLUTE_PATH
```

A run-owned hold fences new work and survives interruption. Drain accounts for
live admissions and all nonterminal Paperboy Nucleus jobs. Keep the hold until
work has settled and the prior or candidate installation is coherent. Release
only the operation's owner; preserve other holds and operator state.

The controlled migration command takes a supported database backup. Retain it
with a compatible release for recovery. There is no legacy database migration
or direct incompatible rollback. Do not copy or restore Nucleus authentication.

## Select the daily schedule

After an authorized installation, select the current installed executable:

```sh
paperboy schedule enable
paperboy schedule status
paperboy schedule disable
```

The single daily binding selects one report kind. To select decision reports:

```sh
paperboy schedule enable --report decisions --annals-config /absolute/decisions.toml
```

This replaces the daily conversation selection. Run `schedule enable` without
source options to select conversations. Ad hoc reports remain independent.
Decision reports use documents accepted by Annals during the requested interval.
The absolute config must identify a decisions library and its expected persistent
ID. Schedule arguments retain this selection across deployment. Doctor with the
same source options checks the read-only Annals starting-cursor operation.

Enable registers and selects the exact `paperboy/daily` Clockwork definition.
It succeeds when Clockwork commits that binding. The schedule starts at local
09:00, has no run-at-load trigger, skips overlap, and limits an activation to
2,100 seconds. It requires a macOS GUI login session. No start-delay or inbox
arrival guarantee is provided. The product-owned schema-two definition declares
`halt-until-approved` and uses the installed Email wrapper for incident mail.

The runner reports exactly 86,400 seconds ending at the most recent local
09:00. It does not replay older missed mornings. Local timezone changes affect
future triggers; daylight-saving changes can produce gaps or overlaps.

Coordinated deployment updates the selected digest and preserves enabled
intent and Clockwork failure incidents. Enable or disable does not clear an incident.
Retain releases pinned by schedules, including disabled selections.

## Failure and recovery

A process lock serializes runs and schedule mutations. Preserve maintenance when
recovery cannot prove a coherent installation. Restore state only with its
matching compatible release. Unknown apply or email outcomes remain uncertain.

Accepted summaries and submission receipts survive later runtime failures.
Resume an interrupted brief with `paperboy run --brief BRIEF_ID`. A terminal
agent failure requires explicit `--retry-agent` for a new job. Ambiguous
admission reuses the exact retained request.

For uncertain email, inspect Resend before using `paperboy reconcile` with an
acceptance receipt or confirmed nonacceptance. Never infer absence from a timeout.
A retained submission receipt does not establish final inbox delivery.

A startup failure, crash, timeout, or nonzero report completion halts the
schedule in Clockwork. Inspect `clockwork incident list paperboy/daily` and
`clockwork incident show INCIDENT_ID`. After repair, explicitly approve future
scheduling with `clockwork binding resume paperboy/daily INCIDENT_ID`. This
does not retry a brief, create a new agent attempt, reconcile an uncertain send,
or reset its message identity. Use the selected Paperboy recovery operation
separately. Maintenance release and deployment never clear the failure halt.

## Privacy and operational limits

Report runs read normal-user conversation history through Conversations or
accepted Krisis documents through Annals and
process it through Nucleus. Final report text leaves for Resend and the personal
inbox provider. The agent has no workspace, local execution, web, or mail tool.
Email's installed wrapper loads credentials. Secrets stay out of Paperboy state,
agent requests, and Clockwork definitions.

One product runner can be active. Agent execution is limited to 1,200 seconds;
total agent wait to 1,800 seconds; Email invocation observation to 180 seconds.
History pages contain at most 100 records. Final bodies contain at most 64,000
UTF-8 bytes. There is no automatic local pruning, unlimited retry guarantee,
source-completeness promise, or language certification. Keep database backups
private; Nucleus and mail-provider retention are separate.

## Deployment configuration and activation

Deployment captures and suspends `paperboy/daily`, initializes absent state or
backs up supported state during configuration, then selects a disabled
definition for the final installed program. It preserves existing schedule,
arguments, environment and output paths. The optional `enabled` deployment
setting overrides saved enabled intent. With no setting, an absent binding
remains absent and a disabled binding stays disabled. Activation follows all
maintenance releases. Existing Clockwork incidents survive every phase.

Recovery repeats configuration under its retained owner. A repeated backup
path is accepted only if its integrity and all retained report, agent and
email-attempt rows match the held Paperboy state.

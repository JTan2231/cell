# EMT worker and installation state

Program publication copies the selected release into fixed regular files beneath
~/Library/Application Support/EMT/install/runtime. Public commands use that runtime
tree; current and previous retain immutable UUID archive selections. Code signing and
runtime path identity are separate from release identity.

New Clockwork definitions use schema 3: they retain the archive release ID, root and
exact hashes, and execute the fixed runtime image. Publication precedes registration.

Before publication, deployment runs the disable transition for owned Clockwork bindings
and waits for their active processes, including an active manual run on a disabled
binding. It restores saved enabled intent after registration; a failed instruction can
leave the owned bindings disabled.

Existing history, delivery records, enabled intent and incident halts retain their
meaning. Retained definitions and wrapper bytes from before this change keep their
legacy execution paths until a new installation or definition selects the runtime image.

Direct install remains limited to uninitialized EMT state. It rejects an enabled worker
binding and runs the disable transition for a retained disabled binding to wait for
active processes before runtime publication.

EMT owns its current-user private configuration, incident and exchange state,
worker admission, maintenance holds, matched program publication.
Clockwork owns `emt/worker` scheduling and failure halts. Nucleus owns agent
execution. Email owns transport, account configuration and credentials.

Use this feature to understand service readiness, activation, maintenance,
installation guarantees and retained state. Use `emt.installation.operate` to
carry out installation, setup, activation or recovery. Read
`emt.incident.respond` for incident, assignment and correspondence meaning.
`emt.quota-notices` supplies the required quota and frozen-notice behavior.

## Interfaces

~~~sh
emt init
emt configure --receiving-domain DOMAIN --cell-root /absolute/cell
emt doctor
emt --json status
emt pause
emt resume
emt worker
emt schedule enable
emt schedule disable
emt schedule status
emt maintenance hold OWNER
emt maintenance status
emt maintenance drain
emt maintenance release OWNER
emt migrate
emt-install install --binary ABS --bundle ABS
~~~

These interfaces operate only the current user's installation. Catalog reads
establish no authority to activate work, read an account, send mail, install
programs or release a hold. Enabling admission requires authorization for
automatic investigation, receiving reads and incident email. Recognized replies
then authorize the one-off intervention described by `emt.incident.respond`.

## Configuration and readiness

The state root is `~/Library/Application Support/EMT`. Init creates schema-one
state and paused configuration. Configure also accepts `--agent-cwd`, `--model`,
`--email-executable` and `--clockwork-executable`. Omitted values remain unchanged.
Changes require paused admission and drained work. Provider executable paths,
Cell root and agent working directory are absolute. Use an existing stable Cell
checkout; deployment worktree paths are not retained as Cell configuration.
The receiving domain must belong to Email's configured receiving account.
EMT stores no credentials.

The default agent cwd is the user's home; Cell source is supplied separately.
Assignments require Nucleus invocation policy version two and
`workspace-unrestricted`; `emt.incident.respond` owns the full execution-authority
contract. The default model is gpt-5.6-terra with medium reasoning; another
configured model must be accepted by Nucleus.

Doctor reads local configuration and schema, SQLite `quick_check`, the Clockwork
feed interface, Nucleus health and Email executable presence. It submits no job
and sends no mail. Nucleus health is an observation at the probe boundary.
Email receiving permission and final delivery remain `not_probed`; executable
presence does not prove either. Status reports retained incident and exchange
counts and uncertain emails, rather than lifetime totals or product health.
Explicit incident and exchange reads can expose private correspondence.

## Admission and schedule

Pause stops discovery of new incidents and replies and disables EMT preference
for newly routed Clockwork notifications. Admitted exchanges continue. Resume
validates configuration, configures Clockwork's EMT route and reopens admission.
Neither operation enables a schedule or clears a Clockwork halt.

The worker runs every 15 seconds without run-at-load. It skips overlap and has a
300-second activation timeout. It does not wait for model completion. Ordinary
Nucleus or Email unavailability reports waiting. Unexpected local-state or
Clockwork-interface failure exits nonzero and can halt `emt/worker`; its alert
always uses Clockwork's basic path, without recursive EMT diagnosis.

Operator pause, disabled scheduling, maintenance holds and failure halts are
separate states. Installation, schedule switch and EMT resume do not clear a
failure halt. Exact incident-bound continuation belongs to Clockwork.
No timer latency, service availability or maximum recovery duration is promised.

## Maintenance and retained state

Holds belong to an exact owner. Hold stops new discovery while drain advances
existing exchanges. Maintenance status reports `protocol_version`, holds,
`drained` and outstanding exchanges. Unknown counts are not zero. Release
removes only the selected owner's hold and does not approve a failure halt.
Restart cannot resume an old agent process. The owning product must inspect
uncertain effects before an attended maintenance operation changes Nucleus.

Migration accepts only schema one and requires drained work. `emt migrate`
checks existing state and returns `schema_version:1`. With no database, it
initializes paused state. It copies no database or configuration. Initialization
can finish an interrupted empty schema or missing empty-state configuration.
It refuses missing configuration when incident or exchange records already exist.

EMT retains correspondence and Nucleus references without automatic deletion.
Preserve `quota-notifications/` with the database and configuration. Nucleus,
Resend and the inbox provider retain separate records under their own authority.
EMT has no activity mirror, operation ledger or credential copy. Ordinary status
and logs omit bodies; explicit reads, model prompts and email expose selected
content. No future support lifetime or retention horizon is promised.

## Matched installation instructions

The installer stages the binary, matching installer and Chancery bundle in an
immutable `cell-install-v4` release. Before initialization, `emt-install install`
accepts `--binary ABS`, `--bundle ABS`, `--home` and `--expected-current`.
This installs bytes without initializing state or selecting a schedule. The
product selector publishes the bundle with its release.

`./deploy.sh emt` executes the declared `emt-install deploy` instruction.
The installer publishes the matched release, initializes or migrates its local
schema-one state, saves the requested configuration, configures the Clockwork
EMT route and publishes the intended worker definition. It uses ordinary
product admission and runner locks. It creates no maintenance hold, drains no
exchanges and performs no application-health or signature audit.

Product setup accepts current configuration fields and an `enabled` boolean.
It does not accept incoming-mail progress changes. Omitted settings preserve
saved values. A fresh deployment uses the default worker schedule with
activation enabled and product pause removed; explicit `paused` or `enabled`
overrides that default. An initialized product's absent binding remains absent
unless activation is explicitly requested. Existing schedules retain their
intended enabled state, and existing failure halts remain in force.

A domain supplied with EMT or selected Email setup takes precedence. Otherwise,
the installer resolves a missing receiving domain through Email `receive settings`.
An empty or ambiguous result requires an explicit domain. This setup instruction
does not inspect received mail to infer account settings.

The executor records instruction completion from the exit status. Failure leaves
completed effects in place. Interruption requires inspection of the retained
instruction log and affected paths before an explicit next operation; it does
not trigger migration replay, rollback or recovery. Direct selector recovery is
unsupported after initialization. Explicit product maintenance remains separate.

## Dependencies and compatibility

Clockwork must support the incident feed and notification handoff before routing
is enabled. Refresh every active generated broker plist to the compatible
release; an old pinned broker ignores EMT claims and must not run while claims
exist. Routing uses a version-one sidecar beside Clockwork's schema-two database.

Clockwork's read-only service checks require compatible Iatreion. EMT supplies
its configured stable Cell root. Preserve Clockwork's `failure-checks.json`,
`notification-checks.json`, routing metadata and incident database during maintenance.
Refresh enabled pinned brokers to the new failure-check contract; an older
broker must not run while the new sidecar exists. Clockwork owns check policy,
notification ownership, scheduling and incident-bound continuation. The shared
threshold delays a new scheduling halt, notification and diagnosis
together. By default, five consecutive failed read-only checks, at least 60
seconds apart, establish the halt and alert eligibility. Later scheduled
activations remain admissible before that threshold. Healthy or inactive checks
clear a pending episode; established halts still require exact approval.

EMT schema, provider release and feature contracts evolve independently.
Installation performs setup without artifact-integrity, state-integrity, or
operational-readiness checks. Ordinary doctor and worker checks remain unchanged.
Installation and indexed documentation do not establish live readiness. No
schema other than one, initialized direct-selector recovery, automatic failed
assignment replacement, sender authentication or final-delivery guarantee is
supported.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time and thread ID, not arguments, output or
outcomes. Internal product calls are excluded. Recording errors do not change
command results.

# Install and operate EMT

EMT's root is ~/Library/Application Support/EMT. Its installer stages the
binary, matching installer and Chancery bundle in an immutable cell-install-v2
release. Clockwork owns emt/worker. Installation, initialization,
configuration, admission and scheduling are separate effects.

## Prepare and activate

After separately authorized installation:

~~~sh
emt init
emt configure --receiving-domain YOUR-RECEIVING-DOMAIN --cell-root /absolute/cell
emt doctor
emt resume
emt schedule enable
~~~

Init creates schema-one state and paused configuration. Configure also accepts
--agent-cwd, --model, --email-executable and --clockwork-executable. Omitted
values remain unchanged. Changes require paused admission and drained work.
The receiving domain must belong to Email's configured receiving account.
EMT stores no credentials.

The default agent cwd is the user's home, with local execution and read-write
workspace access. This is deliberate operational authority. Cell source is
supplied separately. Nucleus must support the configured invocation.

Before EMT resume, deploy Clockwork's incident-feed and notification-handoff
interfaces and refresh all active generated broker plists to that release.
An older pinned broker ignores EMT claims. Routing uses a version-one
metadata sidecar beside Clockwork's unchanged schema-two database. Never
run old brokers while claims exist.

Doctor checks local configuration/schema, SQLite quick_check, the Clockwork
feed interface, Nucleus health and Email executable presence. It submits no
job and sends no mail. Returned Nucleus health is an observation. Email
receiving permission and final delivery remain not_probed.

## Worker and admission

~~~sh
emt schedule status
emt pause
emt schedule disable
~~~

The worker runs every 15 seconds without run-at-load, skips overlap and has a
300-second activation timeout. It does not wait for model completion. Ordinary
Nucleus or Email unavailability is reported as waiting. Unexpected local state
or Clockwork interface failure exits nonzero and can halt emt/worker, whose
alert always uses Clockwork's basic path.

EMT pause, disabled scheduling, deployment holds and failure halts are distinct.
Install, schedule switch and EMT resume never clear a failure halt. Inspect
and approve its exact ID with clockwork binding resume emt/worker INCIDENT_ID.

## Installation and updates

Before initialization, emt-install install accepts --binary ABS --bundle ABS
and shared --home and --expected-current options. It installs bytes without
initializing state, running agents, sending mail or enabling a schedule.

Initialized updates use Cell's maintained coordinator. Disable emt/worker,
hold and drain EMT before Nucleus is held, install the matched release and
provider, verify readiness and explicitly re-enable the schedule.

~~~sh
emt maintenance hold OWNER
emt maintenance status
emt maintenance drain
emt maintenance release OWNER
emt migrate --backup /absolute/private/emt-backup.sqlite3
~~~

Drain advances existing exchanges without discovering more incidents or mail.
Status reports protocol_version, holds, drained and outstanding exchanges.
Unknown counts are not zero. Nucleus restart cannot resume old agent processes.

Migration accepts only EMT schema one. Existing drained state is copied into
the database backup and a companion .config.json file. Backups contain
correspondence. Existing destinations must match and are never overwritten.
With no database, migration initializes paused state and reports backup:null.

Direct selector recovery is unsupported for initialized EMT. Use maintained
coordinator recovery and retain unresolved holds. Backups do not restore
Nucleus or Email records. Never release another operation's hold or silently
resume a halt.

EMT declares Email, Nucleus and Clockwork dependencies. Nucleus maintenance
includes EMT. Shared cleanup recognizes EMT's root and preserves active pins.
See emt.incident.respond for standing authority, agent policy, record meaning,
notification ownership, deadlines and recovery.

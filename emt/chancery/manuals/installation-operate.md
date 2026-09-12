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

Initialized updates use `./deploy.sh emt`. The coordinator captures configuration
and worker intent, disables the worker, holds and drains EMT, installs the matched
release and provider, selects a disabled exact worker definition, and verifies
readiness. It releases admission and restores enabled state. Existing pauses
and failure halts remain. Configuration must be valid before maintenance.
A requester-only update leaves Nucleus admission open. Nucleus replacement
waits for EMT's existing exchanges before holding the service.

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
The coordinator stores these backups in EMT's state directory. The backup
destination must be absolute and separate from the live database and config.
With no database, migration initializes paused state and reports backup:null.

Direct selector recovery is unsupported for initialized EMT. Use maintained
coordinator recovery and retain unresolved holds. Backups do not restore
Nucleus or Email records. Never release another operation's hold or silently
resume a halt.

EMT declares Email, Nucleus and Clockwork dependencies. Nucleus maintenance
includes EMT. Shared cleanup recognizes EMT's root and preserves active pins.
See emt.incident.respond for standing authority, agent policy, record meaning,
notification ownership, deadlines and recovery.

## Deployment setup and recovery

Product setup accepts the existing configuration fields and an `enabled` boolean.
It does not accept incoming-mail progress changes. Omitted settings preserve the
saved values. A fresh deployment uses the default daily/worker schedule, with
activation enabled and domain pause removed after verification. An explicit
`paused` or `enabled` value overrides that default.

Before maintenance, resolve a missing receiving domain through Email's
`receive settings` interface. A domain supplied with this product or the selected
Email setup takes precedence. An empty or ambiguous result requires a supplied
domain; deployment does not inspect received mail to infer account settings.
An initialized product's absent binding remains absent unless activation is
explicitly requested.

Recovery uses the original captured configuration and worker intent. It restores
a disabled exact definition before release and then applies the captured pause
and enabled settings. Fresh initialization's temporary pause is not operator
intent. Existing pause and failure halt evidence survives every phase.

The retained coordinator directory holds this product's migration receipt. It
records the completed schema and original backup digests before configuration
changes. Recovery checks that evidence and reuses the backup; it does not
replace the original backup with already configured state.
EMT initialization can finish an interrupted empty schema or missing empty-state
configuration. It refuses missing configuration when incident or exchange
records exist. Set `cell_root` to an existing stable checkout when the default
`~/rust/cell` does not apply. Deployment worktree paths are not retained as Cell
configuration.

## Shared quota notices

The worker reads Nucleus `GET /v1/quota` without starting an agent. While quota
blocks admission, unadmitted exchanges wait until recovery or their existing
deadline. Expired quota deferrals and `quota_exhausted` attempts retain their
outcome without generating an individual fallback failure email. Retained agent
emails still use the ordinary delivery path. Unrelated incidents retain their
normal handling. Existing pauses and failure halts are not cleared.

Each new shared condition freezes one deterministic email in the private
`quota-notifications/CONDITION_ID.json` record under EMT's state root. Email is
invoked directly with key `emt/quota/CONDITION_ID`; no Nucleus job authors or sends
this notice. The same frozen payload has at most two transport invocations,
at least five minutes apart and within 23 hours of the first attempt. A receipt
ends sending. An unresolved exhausted send remains uncertain for inspection.
Keep this directory in backups; do not delete records to retry delivery.
A missing or unavailable quota observation postpones new model work while frozen
email delivery continues. An old daemon's quota-endpoint 404 permits rollout
without a quota gate. Worker recovery and operator pause stop discovery of new
quota notices but allow frozen notice delivery.

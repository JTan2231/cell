# Mentor installation and scheduled operation

Mentor is the Cell product that owns daily problem delivery, incoming-answer
processing, and critiques. Its program name is `mentor`. The separate desktop
Mentor app remains the authoring source for the bundled exercises and owns its
own desktop state.

## Owned paths

All mail-service state is under
`~/Library/Application Support/MentorMail/`. The mail service does not open or
modify the desktop app's `~/Library/Application Support/Mentor/` directory.

| Path under MentorMail | Owner and purpose |
| --- | --- |
| `mentor.sqlite3` | Mentor configuration, immutable imported content, delivery records, and temporary processing data |
| `deployment-maintenance/` | Mentor's shared Cell admission and maintenance gate |
| `runner.lock` | Serialization of worker and administrative mutations |
| `install/releases/<digest>/` | Immutable `cell-install-v2` program and provider release |
| `install/current` and `install/previous` | Shared installer release selectors |
| `schedules/<digest>.toml` | Immutable source manifests registered with Clockwork |
| `logs/worker.stdout.log` and `logs/worker.stderr.log` | Private worker output |

The installer owns the `~/.local/bin/mentor` and
`~/.local/bin/mentor-install` command selectors and the installed
`~/Library/Application Support/Chancery/providers/mentor` provider selector.
Clockwork owns the `mentor/worker` binding, activation records, durable failure
halt, incident notification, and generated LaunchAgent. Mentor owns its
definition and declares `halt-until-approved`. Mentor does not write a
LaunchAgent itself.

## Install program bytes

`mentor-install` delegates release construction, publication, ownership proof,
locking, and selector recovery inside the coordinator to the shared Cell
installer. A release stages the Mentor executable, matching installer, and
`mentor/chancery` provider together. The corpus is embedded in the executable.
No predecessor installation format is supported, and foreign selectors are not
taken over.

A direct first installation accepts absolute paths:

```sh
/absolute/path/to/mentor-install install \
  --binary /absolute/path/to/mentor \
  --bundle /absolute/path/to/cell/mentor/chancery
```

This installs program bytes and documentation. It does not initialize the
database, enable a schedule, run a model, or send mail. It is supported only
before the Mentor database exists. `--home ABS` and
`--expected-current absent|releases/HASH` are shared installer options.
`mentor-install inspect` reports installed selection. `verify` and
`verify-release` are the shared installed-artifact operations described by
`mentor-install --help`.

Initialized installations use the Cell maintained deployment coordinator.
Mentor orders selected Email, Nucleus, and Clockwork releases before itself.
Nucleus replacement holds installed Mentor admission. A Mentor-only deployment
leaves Nucleus admission open. Cleanup uses the `MentorMail` installation root.
The coordinator captures worker and configuration state, disables the worker,
holds and drains admission, runs the candidate's `migrate --backup ABS`, and
publishes matched artifacts. Configuration selects a disabled exact worker
definition. After doctor and hold release, activation restores captured enabled
state. Existing operator pauses and failure halts remain. Fresh state uses
Mentor's default daily time and enables its worker after verification unless setup
requests otherwise. The receiving domain comes from supplied settings or Email's
sole configured receiving domain.
Maintenance responses use `protocol_version: 1`, `holds`, and `drained`.
Domain pause is independent of deployment holds and Clockwork's failure halt.
Disabling, switching or re-enabling the binding preserves that halt.

Direct `mentor-install recover` is unsupported: selecting older bytes alone
cannot prove compatibility with current database state. Coordinator recovery
retains its captured installation evidence and run-owned maintenance hold.
It does not restore a database backup automatically. Keep an unresolved hold
in place and use the documented coordinator recovery route rather than
editing selectors or database files.

## Initialize and configure

Initialization is explicit and starts paused:

```sh
mentor init
mentor configure \
  --time 09:00 \
  --timezone America/Chicago \
  --receiving-domain YOUR-RECEIVING-DOMAIN \
  --email-executable /absolute/path/to/email
mentor doctor
```

To start on a future local date, configure it before resuming:

```sh
mentor configure --first-delivery-date 2026-09-08
```

The date uses the configured time zone, and delivery still waits for 09:00 or
the configured daily time. With no first date, enabling after the daily time
makes the current date eligible. `mentor status` exposes the selected first
date as `first_delivery_date`, or `null` when absent.

Email owns the recipient, credentials, provider requests, and incoming mail
transport. The receiving domain must route to that configured Email account.
Mentor's configuration contains no mail-provider credentials. `doctor` does
not invoke the grading model or send a message; installed artifact health,
configuration, scheduling, and provider readiness remain distinct outcomes.

## Enable daily delivery and replies

After configuration, these are separate explicit actions:

```sh
mentor resume
mentor schedule enable
```

`resume` removes the product's operator pause. Schedule enable leaves that
pause setting unchanged. It requires initialized, valid configuration and an
installed release, then generates and registers an immutable Clockwork
definition and selects it through `mentor/worker`.

The definition runs every 60 seconds with `run_at_load = false`, a 90-second
activation timeout, and skipped overlapping activations. It launches the
exact native executable under `install/releases/<digest>/bin/mentor` with
`--json worker`, using MentorMail as its working directory. The launch
environment contains only `HOME`. Standard output and standard error use
separate private files. No shell profile is sourced by Mentor's launcher.
The executable hash comes from the verified selected release.

Daily delivery time, time zone, exercise selection, incoming-message progress,
and payload safety remain Mentor's responsibilities. Clockwork owns the
configured response to an abend. The schema-two definition defaults to halting
until explicit approval and uses the installed Email wrapper for the incident
notification. A failure stops the current pass before further work. A timeout
halts scheduling; a missed activation alone is not a detected abend. Clockwork
does not decide whether a problem is due or an answer has been graded.

Inspect and continue an exact incident through `clockwork.schedule.operate`.
Continuation does not reset Mentor's deadlines or replace a model job or frozen
message. `mentor cleanup` remains available while halted to expire temporary
content and cancel expired model jobs without admitting practice work. No
automatic cleanup occurs while the worker is halted; exact wall-clock deletion
and cancellation deadlines are not guaranteed.

```sh
mentor schedule status
mentor pause
mentor schedule disable
```

`pause` changes the product's domain pause. Schedule disable stops future
Clockwork activations, preserves retained selection and history, and does not
terminate an already running worker. It leaves the pause setting unchanged.
Disable is available under a maintenance hold, but it still shares the runner
lock with active work. Enable must respect every hold.

## Updating an existing installation

Run `./deploy.sh mentor` from the Cell checkout. The coordinator owns suspension,
backup, installation, worker pin selection, verification and activation. No
separate schedule command is required for an ordinary update. A stopped run
retains unresolved recovery evidence for the next deployment command.

Prior release bytes, definitions,
and activation history remain retained. Schedule enable can reuse an already
registered identical definition after an interrupted attempt. A mismatched
existing definition, unsafe manifest, or unavailable provider is reported as
an error; it is not treated as successful scheduling.

## Development gate

`mentor/ci.sh` delegates to the shared pipeline using
`pipeline/products/mentor.sh`. The descriptor owns package checks, provider
validation, and release-binary expectations. `mentor/release.sh` enters the
shared release and deployment workflow; it is not a build shortcut. Run gates,
builds, release operations, or live configuration only when that work is in
the authorized scope.

## Readiness and content-free backup boundary

`doctor` inspects only local installation/state compatibility and Email
executable presence. It reports Email API/receiving permission, Nucleus
execution readiness and live delivery as `not_probed`; it does not contact
those services to prove readiness. No model job or email is created.

Maintenance uses `mentor maintenance hold OWNER`, `status`, `drain`, and
`release OWNER`. Drain requires a hold and advances one bounded pass of existing
work. Its `data.maintenance` observation has `protocol_version: 1`, `holds`,
`drained`, and `outstanding_work_record_count`. Unknown outstanding counts are
not zero. Release requires drained state and removes only that owner.

`mentor migrate --backup PATH` refuses unresolved work/cancellation and any
pending answer, request or outgoing payload text before copying an existing
database. The backup is a private 0600 file. An existing destination must match
the drained database exactly and is never overwritten. With no database yet,
migration initializes paused schema-one state and reports `backup:null`. The
backup and Mentor content cleanup do not remove Nucleus or mail-provider
records.

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

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.

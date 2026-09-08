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
Clockwork owns the `mentor/worker` binding, its activation records, and its
generated LaunchAgent. Mentor does not write a LaunchAgent itself.

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
Mentor's adapter orders selected Email, Nucleus, and Clockwork releases before
Mentor. Its maintenance closure includes Nucleus, and Nucleus includes Mentor
among the requesters it must hold. Deployment cleanup uses the `MentorMail`
installation root.
Disable `mentor/worker` before deployment; an enabled binding makes installer
inspection and publication refuse the change. The coordinator uses Mentor's
`maintenance hold`, `status`, `drain`, and `release` interface, runs the
candidate's `migrate --backup ABS`, publishes matched artifacts, and calls
`doctor` before releasing its exact hold. Maintenance responses use
`protocol_version: 1`, `holds`, and `drained` in the JSON data envelope.
Domain pause is independent of deployment holds.

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
and recovery decisions remain Mentor's responsibilities. Clockwork supplies
periodic activation and does not decide whether a problem is due or an answer
has been graded. A timeout or missed activation can be followed by a later
pass over retained work; completion is established by Mentor's records.

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

1. Explicitly disable `mentor/worker` and allow admitted work to drain through
   the maintained deployment procedure.
2. Use the Cell coordinator to install the candidate and matching provider
   with a run-owned hold and schema-one migration backup.
3. Resolve any deployment failure before releasing the hold.
4. Explicitly run `mentor schedule enable` after the deployment completes.

Re-enabling constructs a definition for the newly selected immutable release.
The installer never silently starts a schedule or redirects an old pinned
definition to different executable bytes. Prior release bytes, definitions,
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

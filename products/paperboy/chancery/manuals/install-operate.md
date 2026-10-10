# Install Paperboy and select production schedules

Program publication copies the selected release into fixed regular files beneath
~/Library/Application Support/Paperboy/install/runtime. Public commands use that
runtime tree; current and previous retain immutable UUID archive selections.
Code signing and runtime path identity are separate from release identity.

New Clockwork definitions use schema 3: they retain the archive release ID, root
and exact hashes, and execute the fixed runtime image. Publication precedes
registration.

Before publication, deployment runs the disable transition for owned Clockwork
bindings and waits for their active processes, including an active manual run on
a disabled binding. It restores saved enabled intent after registration; a
failed instruction can leave the owned bindings disabled.

Existing history, delivery records, enabled intent and incident halts retain
their meaning. Retained definitions and wrapper bytes from before this change
keep their legacy execution paths until a new installation or definition selects
the runtime image.

Use this operation to install Paperboy, prepare its production definitions, and
control exact production schedules through Clockwork. Installation, schedule
activation, and script execution are separate effects. Read
`paperboy.report.send` for the production definitions, renderer output, email
result, privacy, and execution limits.

## Prepare and deploy

Cell deployment uses CI and Telete with committed source and an explicit product
selection. Run from the Cell root:

```sh
./ci.sh submit COMMIT --deploy paperboy
```

Select any required Clockwork or Email update with another `--deploy PRODUCT`.
Telete validates and prepares signed artifacts, then runs product-owned setup
instructions to select matching Paperboy program and Chancery provider bytes.
Paperboy uses the installed Clockwork and Email interfaces. It invokes neither
Nucleus nor source-specific products. Verify the retained Telete job and
deployment outcome. CI and Telete are the sole deployment route.

Installation accepts no product settings. It uses the default
production-definition file; an alternate interactive `--manifest` selection does
not change this scope. Installation initializes a missing default
production-definition file and applies its configured schedule snapshots. New
production schedules stay disabled. Existing schedules retain their enabled
intent; removed schedules are disabled. Installation does not execute a
renderer, send its output, or clear a Clockwork failure incident. An empty
production-definition file selects no production schedule.

Installation succeeds when its declared setup instructions complete. It does not
establish source access, Email authentication, renderer correctness, or future
schedule delivery. The executor retains completed effects on failure; it
performs no automatic rollback or application recovery. Inspect the retained
deployment receipt and schedule state before an explicit new attempt.

Direct installer publication and selector-only recovery are unavailable. Use the
coordinator for program selection. Inspect maintained installation separately
with `paperboy-install inspect`; that read establishes no operational readiness.

## Initialize and inspect production definitions

The default production-definition file is `~/Library/Application
Support/Paperboy/paperboy.toml`. Select another production-definition file with
a global absolute `--manifest` path:

```sh
paperboy init
paperboy list
paperboy doctor
paperboy --manifest /absolute/paperboy.toml list
paperboy schedule status
paperboy schedule status daily-report
```

`init` writes version-one empty production definitions only when the selected
production-definition file is absent. It validates and preserves existing files.
`list` reports the configured productions. Doctor validates the production
definitions and required executable paths without executing scripts or sending
email. A valid path does not prove that the renderer or Email can complete its
work.

Schedule status reads Clockwork's retained binding metadata. It reports selected
definitions, enabled intent, and failure-halt evidence. It does not prove that
launchd will deliver a future trigger or that a report was delivered.

Paperboy's aggregate `status-snapshot` describes production definitions and
local readiness. It is read-only evidence, not a renderer execution, email test,
or complete report for each dynamic Clockwork production binding. Its aggregate
unit is `paperboy/productions`; individual Clockwork binding IDs remain
`paperboy/PRODUCTION_ID`.

## Apply schedule snapshots

After editing the selected production-definition file:

```sh
paperboy apply
paperboy --manifest /absolute/paperboy.toml apply
```

Apply validates the entire production-definition file and registers all
immutable definitions before changing binding selection. Each production maps to
`paperboy/PRODUCTION_ID`. The production ID is stable across changes to command,
subject, or schedule. Renaming it creates another binding and retires the prior
ID when it is removed.

The selected production-definition file is the complete desired production set
for the `paperboy/` namespace. `--manifest` changes the configuration source,
not that namespace. Applying another production-definition file disables
bindings absent from that file.

Each activation definition captures the installed Paperboy release, production
ID, renderer argv, subject, absolute Email wrapper path, schedule, and literal
launch context. The scheduled runner uses this snapshot without reading the
production-definition file again. Apply is required to publish
production-definition edits to scheduled execution. Renderer file bytes and
source data are not frozen by the snapshot.

Apply preserves enabled intent for existing bindings. It selects new bindings in
a disabled state. It disables removed bindings while retaining their selected
definitions, incidents, and history. An identical selection needs no transition.
Apply never executes a renderer or activates a new production.

A dedicated product schedule lock serializes Paperboy schedule mutations.
Clockwork serializes each binding transition and activation. Different
productions can run concurrently. Clockwork skips overlap for the same scheduled
binding. Manual execution does not become a Clockwork activation record.

Applying several production schedules is not atomic. A later transition can fail
after earlier bindings have changed. Inspect status, correct the reported
problem, and apply the same production-definition file again. Paperboy does not
discard prior transition evidence or undo completed selections. Concurrent
direct same-user Clockwork mutations are outside Paperboy's serialized
operation.

## Enable or disable a production schedule

After an authorized apply, explicitly enable a selected production schedule:

```sh
paperboy schedule enable daily-report
paperboy schedule status daily-report
paperboy schedule disable daily-report
```

Enable and disable require a production ID. Status can select one ID or show the
Paperboy bindings. Enabling permits future scheduled renderer executions and
real email sends. Disabling prevents new scheduled admission; Clockwork settles
an existing activation under its supported transition rules. Disabling retains
selection, history, and incidents. It does not cancel an independent manual run.

The schedule has no run-at-load trigger. Interval values are whole seconds;
calendar values use the host's local hour and minute. Clockwork uses
current-user LaunchAgents and requires a macOS GUI login domain. Sleep, wake,
login, clock, and timezone changes affect delivery. Paperboy promises no maximum
trigger delay, catch-up count, or inbox arrival time.

Each activation has a 1,380-second outer timeout and `overlap = "skip"`. The
installed Paperboy runner uses its product root as the Clockwork working
directory, then executes the renderer from its executable's parent directory.
Product outputs append to private files under `~/Library/Application
Support/Paperboy/logs`: `PRODUCTION_ID.stdout.log` and
`PRODUCTION_ID.stderr.log`. These logs contain Paperboy result metadata and
bounded error diagnostics. Captured renderer stdout is not copied into them.
Paperboy prepares definition-registration TOML files under
`~/Library/Application Support/Paperboy/schedule-definitions` and removes them
after successful registration. A failed candidate can remain for diagnosis.
These files contain command metadata, not rendered bodies or pending sends.

## Inspect a failure and approve future scheduling

Each Clockwork activation definition declares `halt-until-approved`. A startup
failure, crash, timeout, or nonzero Paperboy completion ends that activation.
Clockwork permits later activations during its pending service-health episode.
By default, five failed read-only checks, at least 60 seconds apart, establish
the halt and alert eligibility. Healthy or inactive checks clear pending
episodes. Read `clockwork.schedule.operate` for the complete installed failure
policy.

Inspect the exact binding and incident:

```sh
clockwork incident list paperboy/daily-report
clockwork incident show INCIDENT_ID
```

Resolve the cause before explicitly approving future admission:

```sh
clockwork binding resume paperboy/daily-report INCIDENT_ID
```

Approval clears that incident's halt. It does not enable a disabled binding,
execute the renderer, or retry a prior email. Reinstallation, apply, enable, and
disable preserve established incidents.

Paperboy keeps no saved body or structured send-recovery ledger. Logs can retain
result metadata, including an accepted provider message ID. Email timeout or
interruption can leave acceptance unknown. Inspect Resend before explicitly
rerunning a production when a duplicate matters. A new production run executes
the renderer again; it is not recovery of the earlier payload. Paperboy provides
no preview, saved report, retry, reconciliation, or maintenance CLI.

## Privacy and command usage

Treat production commands, subjects, paths, schedule metadata, and logs as
private local data. A renderer runs with normal-user access and owns its source
and configuration choices. Email loads its credentials and submits only
successful nonempty output. The product stores no rendered body or acceptance
ledger. Clockwork retains activation and incident metadata; its notification
path uses its own supported Email contract.

After each installation or update, run `paperboy --register-usage`. This records
command inventory without executing product work. CLI usage recording requires a
nonempty `CODEX_THREAD_ID`. Chancery's private journal records command identity,
time, and thread ID, not arguments, output, or outcomes. Internal product calls
are excluded. Recording errors preserve command results. Product runtime does
not invoke the Chancery catalog.

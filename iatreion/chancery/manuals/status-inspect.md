# Report Cell operational status

Use Iatreion to construct one bounded report from the operational units declared
by a selected Cell checkout.

```sh
iatreion report /Users/joey/rust/cell
iatreion report /Users/joey/rust/cell --json
iatreion show annals/inbox --root /Users/joey/rust/cell --json
```

The checkout supplies the complete expected inventory. Usher reads each literal
descriptor without executing it. Iatreion invokes only the declared executable
selector basename beneath the selected command directory. It supplies the fixed
arguments `status-snapshot --json` and closes stdin.

## Interpret the report

Each operational unit retains intent, admission, activity, readiness, evidence,
and inspection references. Admission can carry operator pause, maintenance,
and failure halt together. Evidence keeps runtime outcome, domain outcome, and
latest domain success separate. A process exit never establishes product domain
success.

Iatreion derives four display groups. A known failure or blocked prerequisite
needs attention. Proven retired, disabled, operator-paused, or maintenance-held
work is intentionally inactive unless a simultaneous failure needs attention.
Missing required evidence is unknown. Admitted, locally ready active or
on-demand work is operating; running and idle remain distinct.

The report's start and end timestamps describe a non-atomic observation
interval. Event timestamps describe their source events. Old events do not make
a fresh observation stale. A stale heartbeat does not become fresh because a
probe read it now.

## Failure and recovery

A missing declaration, executable, expected unit, unsupported schema, invalid
response, timeout, output overflow, failed probe, or stale observation remains
an explicit unknown result. Iatreion never hides the product and never uses a
prior green result.

Use an attention row's Chancery capability ID and exact incident or record ID
to inspect the owning system. The report does not authorize a repair, retry,
schedule continuation, network canary, deployment, or disclosure.

## Effects and limits

Iatreion keeps the report in memory and exits. It has no database, daemon,
schedule, cache, model call, network check, alert, repair, or retry.

Default bounds are five seconds for the report, two seconds per probe, eight
concurrent probes, and one MiB for each probe output stream. Process scheduling
can delay termination beyond a nominal timeout; these are collection bounds,
not hard real-time guarantees.

The output contains operational IDs, counts with units and scope, timestamps,
bounded reason text, and inspection references. It does not include raw probe
stderr, prompts, logs, credentials, email bodies, or retained document bodies.

`report` and `show` exit zero after constructing the requested report, even when
it contains stopped, blocked, or unknown units. Invalid invocation, unavailable
inventory, and unknown or ambiguous selections exit 2.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.

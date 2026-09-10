# Operational status contract

Iatreion answers which declared Cell operational units are operating, need
attention, are intentionally inactive, or have unknown coverage. It constructs
one in-memory report and retains no result.

## Sources and authority

The selected checkout supplies the expected product and unit declarations.
Usher parses those declarations as data. Each installed product command owns
its `status-snapshot --json` observation. The product remains authoritative for
its domain state and success rules. Clockwork remains authoritative for
scheduled activation and failure halts. Nucleus remains authoritative for
agent execution. Chancery remains authoritative for installed documentation
contracts and compatibility.

Iatreion validates, joins, classifies, and renders these observations. It does
not convert runtime completion into domain success.

## Collection

`iatreion report` uses a five-second report budget, a two-second per-probe
budget, and at most eight concurrent probes by default. These are collection
bounds under ordinary process scheduling, not real-time guarantees. Options can
reduce or increase the bounds for an attended local invocation.

Iatreion invokes one executable selector basename beneath the selected command
directory. It supplies only `status-snapshot --json`, closes stdin, captures
bounded output, and never interprets stderr as status evidence. It makes no
network or model request.

A missing declaration, executable, unit, incompatible schema, invalid response,
timeout, failed probe, or stale observation remains explicit unknown coverage.
Iatreion never reuses a prior result.

## Meaning

Each unit keeps four independent dimensions:

- `intent` states whether the unit is active, on-demand, disabled, retired, or
  unknown.
- `admission` states whether the unit accepts work. Its reasons can include
  operator pause, maintenance, and failure halt at the same time.
- `activity` states whether the unit is running, idle, stopped, not applicable,
  or unknown.
- `readiness` describes only the named locally verifiable prerequisite scope.

Evidence counts name their records, units, and scope. Runtime outcome, domain
outcome, and latest domain success remain separate. Observation time says when
the source was read. Event time says when an outcome occurred.

The human groups are derived presentation. A known failure or blocked
prerequisite needs attention. Proven retired, disabled, operator-paused, or
maintenance-held work is intentionally inactive unless a simultaneous failure
still needs attention. Missing required evidence is unknown. Locally ready and
admitted active or on-demand work is operating; activity remains visible.

## Effects and privacy

The reporter and probe contract are read-only. A probe must not initialize,
migrate, repair, reconcile, refresh credentials, claim notifications, run work,
or change permissions. Probes expose counts, identities, timestamps, bounded
reason text, and inspection references. They expose no prompt, log body,
credential, email body, retained document, or other private content.

`report` and `show` exit zero when they construct the requested inventory-based
report, including reports with blocked, stopped, or unknown units. Invalid
invocation, unavailable inventory, and unknown or ambiguous selections exit 2.

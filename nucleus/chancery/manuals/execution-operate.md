# Operate Nucleus agent execution

Nucleus coordinates constrained Codex execution for local applications under
one user. It owns admission, one supervised harness attempt per job, the global
eight-slot scheduler, cancellation, managed authentication, isolated static
API-key job credentials, exact harness output, and the durable requester-tool
mailbox. Managed authentication uses one authoritative credential. Nucleus has
no project registry or workflow engine. Applications own their domain results.

## Choose this capability

Use this capability to inspect Nucleus readiness or its authenticated account,
submit an exact version-one job request, inspect or follow a job, request
cancellation, perform attended authentication recovery, or operate the macOS
user service.

Do not route a generic request here merely because it mentions a job, agent, or
model. Todo, Annals, and other requesters remain authoritative for the
work that motivated their Nucleus jobs. Ordinary work in the current Codex
session normally needs no Nucleus job at all.

## Supported interfaces

The installed manual is version-matched and does not contact the daemon:

```sh
/Users/joey/.local/bin/nucleus manual
```

Start diagnosis with supported reads:

```sh
/Users/joey/.local/bin/nucleus health
/Users/joey/.local/bin/nucleus service status
/Users/joey/.local/bin/nucleus account --wait 0
/Users/joey/.local/bin/nucleus jobs list --state accepted
/Users/joey/.local/bin/nucleus jobs list --state running
/Users/joey/.local/bin/nucleus jobs list --state waiting-on-requester
/Users/joey/.local/bin/nucleus jobs list --state failed
```

Iatreion uses `nucleus status-snapshot --json`. It reads health and attempts a
maintenance-detail read for at most 250 ms. It reports daemon readiness,
admission, and execution capacity without reading account usage, job content,
or logs. Missing maintenance detail remains an explicit incomplete observation.

`health` is strict: it prints the readiness document but exits nonzero unless
the daemon is compatible, authenticated, and accepting work. It also reports
the configured `maxActiveJobs=8`, live `activeJobs`, and live `availableSlots`.
An `authentication_busy` account result means the broker cannot grant
the read while exclusive authentication maintenance or attended login is in
progress; an ordinary active job does not by itself block an account read or
establish that the credential is invalid.

One exact request is submitted from a file or standard input:

```sh
/Users/joey/.local/bin/nucleus jobs submit <REQUEST_JSON>
/Users/joey/.local/bin/nucleus jobs show <JOB_ID>
/Users/joey/.local/bin/nucleus jobs logs <JOB_ID>
/Users/joey/.local/bin/nucleus jobs logs --follow <JOB_ID>
/Users/joey/.local/bin/nucleus jobs cancel <JOB_ID>
```

The job ID is the idempotency key. If submission is uncertain, resend only the
byte-equivalent typed request with the same ID. A new attempt needs a new ID and
the requester's decision that it is safe.

Completed structured output is reconstructed from the retained attempt's
supported API-key or managed-authentication startup sequence and its correlated
terminal messages. Reading an old completed job after a decoder repair can
recover its thread ID, turn ID, and final message without a new attempt or a
change to its raw observations or terminal state. Today's authentication mode
does not select a historical decoder. Missing or conflicting startup evidence
remains missing output; a requester still owns any decision to retry its work.

## Effects and authority

Submitting can invoke Codex and consume account allowance. A job receives one
attempt; Nucleus never creates an automatic retry. At most eight attempts are
active across all requesters. A newly admitted job stays `accepted` with a
`pending` attempt while it waits for a slot. Its wall-clock timeout starts only
after that slot is acquired, and an attempt in `waiting_on_requester` keeps the
slot until the process and terminal cleanup finish. Dynamic tool calls may cause
requester-owned mutations only after that requester validates and services
them. A successful tool result or Nucleus completion still does not establish
application success.

Nucleus does not detect overlapping working directories or mutation targets.
Concurrent `read-write` jobs require disjoint directories or worktrees, or
requester-owned serialization.

Cancellation targets one exact job. Repeating the request is idempotent. It
does not remove the job, output history, or a requester mutation already
committed.

The authentication broker keeps one authoritative managed credential beneath
Nucleus private state; static API-key jobs instead receive isolated credential
snapshots without copy-back. Jobs and account reads may overlap; canonical
refresh is serialized, staged away from the authoritative file, and atomically
promoted so credential generations move only forward. A started refresh
survives cancellation of its requesting job. Account reads also use private
staging and finish safe credential reconciliation after requester cancellation
or timeout. Attended login is exclusive, does not begin until active job and
account sessions have ended, and promotes only a validated successful staged
login:

```sh
/Users/joey/.local/bin/nucleus auth login --device-auth
/Users/joey/.local/bin/nucleus account --wait 0
/Users/joey/.local/bin/nucleus health
```

Quiesce requesters before login or service work when active attempts must remain
uninterrupted. Nucleus provides durable deployment admission holds owned by run
IDs and reports drain status. A service restart terminates the daemon; startup
marks unfinished attempts `lost`. Service uninstall removes the user service and
installed binaries and retains state and logs.

## Success and recovery

For a runtime read, success is the requested supported Nucleus response. For a
direct job, success is admission and the intended runtime observation. When a
requester is involved, separately inspect its database or filesystem result.

If Nucleus cannot connect, inspect service status and
`~/Library/Logs/Nucleus/nucleusd.stderr.log`; do not fall back to an
uncoordinated direct Codex invocation. If a job is waiting on the requester,
inspect the pending mailbox call and requester state rather than inventing a
result. If an attempt is lost, timed out, or failed after a domain commit,
inspect domain state before considering any replacement attempt.

## Privacy

`~/Library/Application Support/Nucleus/` is sensitive. Its database may
contain complete prompts, tool arguments and results, source content, and exact
app-server stdout. Its Codex home contains authentication material. The Unix
socket has no application-level authentication; current-user filesystem
ownership and permissions are the trust boundary.

For backup, migration, deployment, exact harness compatibility, and detailed
recovery ordering, use `nucleus manual` as the current authority.

## Deployment admission and verification

The deployment coordinator owns one durable hold through:

```sh
nucleus maintenance hold RUN_ID
nucleus maintenance status
nucleus maintenance health RUN_ID
nucleus maintenance release RUN_ID
```

The daemon stores holds beside its configured database in
`deployment-maintenance/`. The standard path is
`~/Library/Application Support/Nucleus/deployment-maintenance/`. Holds survive
CLI or daemon exit. A hold prevents every new HTTP job submission, including
unknown or direct callers, with HTTP 503 `deployment_maintenance`. An exact
existing request can still be rediscovered. Existing accepted jobs continue
through the scheduler; job reads, cancellation, output, and requester mailbox
responses remain available. Requesters must first finish their admitted
multi-job workflows before Nucleus is held.

The JSON status has `protocol_version: 1`, `holds`, `drained`, and
`nonterminal_jobs`. Drain requires no admission guard, no accepted/running/
waiting job, and no terminal cleanup still supervised by the daemon. Releasing
one run ID leaves all other holds intact. No expiry silently reopens admission.

Ordinary health remains strict and reports `acceptingJobs: false` while held.
`maintenance health RUN_ID` proves the sole matching owner, zero unfinished
jobs and active slots, drained admission guards, authenticated credentials, supported protocol, and a
ready exact harness. The health document is returned unchanged. The typed
client provides `health_for_deployment`; this exception is solely for
installation readiness, never ordinary requester admission. Only the service
installer holding its own exclusive activity guard may account for that guard
locally; the public health proof requires all guards drained.

The HTTP surfaces are GET `/v1/maintenance` and POST
`/v1/maintenance/{hold,release}`. Each POST accepts exactly
`{"run_id":"OWNER"}` and returns maintenance status. The typed client owns
these request/response types.

The Rust `nucleus-install` executable is sealed beside the matching tested CLI
and daemon. Its `install --binary ABS --daemon ABS --codex ABS --bundle ABS`
command uses shared immutable `cell-install-v2` packages and invokes the
existing Nucleus-owned Rust service installer. Public CLI and daemon copies
remain service-owned so its captured prior binaries and schema rollback
evidence are preserved. The predecessor format-1 package remains verifiable.
`inspect` and `verify-release ABS` are read-only; they never execute a retained
installer or restore authentication.

The macOS deployer accepts `--expected-current absent|releases/HASH` and checks
it under the product update lock before selector mutation. With
`CELL_DEPLOYMENT_RUN_ID`, service install/restart requires the sole drained
hold and retains an exclusive activity guard through replacement and health
verification. The existing guarded database/credential rollback rules remain.

Daemon startup retires terminal records from the removed built-in deployment
probe. Retirement is limited to requester `nucleus-deployment`, label
`Verify Nucleus deployment`, and the former deterministic job ID derived from
that requester ID. It removes only those jobs, their attempts, and raw output;
children, tool calls, or unfinished work block retirement. Ordinary job history
and shared schemas remain unchanged. This is not a general pruning API.

## Output selection

Health, account, submission, job show, log, mailbox, cancellation, and service
results retain protocol-one meaning. `jobs status` returns runtime and requester
identity, current attempt state and ID, pending call IDs and names, final-output
availability, and terminal reason and message.

`jobs wait --timeout 60` returns one terminal or timeout observation. A timeout
does not cancel the job. Status reads mailbox and job state in sequence, without
an atomic snapshot. Initial read errors remain errors. `jobs list` defaults to
20 and retains its continuation behavior.

## Coordinated first installation and interrupted cutover

A fresh coordinated installation accepts `codex_bin` and `codex_home` in
Nucleus's deployment settings. Both paths are absolute. `codex_bin` must be the
exact supported Codex version. `codex_home` identifies an existing authenticated
home; settings never contain credential bytes. If Nucleus already owns valid
authentication, installation preserves it. Existing deployments retain their
captured harness and do not import another credential home.

The installer persists the run's local admission hold before a fresh daemon
exists. It starts the service under that hold so dependent products can finish
configuration. The configure phase proves the live harness, authentication,
protocol and drained capacity. Admission opens only at group release.

Before selector or service replacement, the installer writes private
`service-cutover.json` with its owner, prior package, candidate and harness.
Recovery uses this journal to select and reinstall the exact candidate through
`nucleus service recover --daemon ABS --codex ABS` with the recorded
`CELL_DEPLOYMENT_RUN_ID`. This controlled restart establishes which executable
is resident; matching files and a health response alone are insufficient.
The service must have its sole drained hold. If it is stopped, recovery reads
the database without migration and requires every retained job and attempt to
be terminal. It does not cancel or retry requester work.

Recovery can import the recorded authentication source only if Nucleus's owned
authentication file is absent. It never rolls back a credential or database.
A schema or service failure keeps the candidate and journal for recovery.
Unknown ownership or unfinished jobs keep admission held. Successful recovery
removes the cutover journal after held live health and exact program-copy checks.

Deployment settings accept only `codex_bin` and `codex_home`, both strings.
Unknown keys or values of another type fail inspection before admission holds.

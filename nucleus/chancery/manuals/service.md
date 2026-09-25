# Service lifecycle and maintenance

Nucleus runs as a current-user macOS service. It owns its private state, socket,
program installation, readiness, and durable maintenance holds. Use this feature
to interpret service health, admission, drain, restart, installation, and recovery.
Use `chancery show nucleus.execution.operate` for ordered installation, backup,
restore, and service-control procedures. Read `nucleus manual` for shared
requester coordination and recovery order before work that can interrupt it.

## Interfaces and access

```sh
nucleus health
nucleus service status
nucleus status-snapshot --json
nucleus maintenance hold RUN_ID
nucleus maintenance status
nucleus maintenance health RUN_ID
nucleus maintenance release RUN_ID
nucleus service restart
nucleus service uninstall
```

```text
GET  /v1/health
GET  /v1/maintenance
POST /v1/maintenance/hold
POST /v1/maintenance/release
```

| Item | Default path |
| --- | --- |
| CLI | `~/.local/bin/nucleus` |
| Daemon | `~/.local/libexec/nucleusd` |
| Service | `~/Library/LaunchAgents/org.nucleus.daemon.plist` |
| Socket | `~/Library/Application Support/Nucleus/nucleus.sock` |
| Database | `~/Library/Application Support/Nucleus/nucleus.db` |
| Credential home | `~/Library/Application Support/Nucleus/codex-home/` |
| Program releases | `~/Library/Application Support/Nucleus/install/releases/` |
| Logs | `~/Library/Logs/Nucleus/` |

Treat state, credentials, logs, and backups as private. Database and mailbox
content can include prompts, source text, and tool values. Socket access uses
local ownership and permissions; protocol 1 has no TCP listener or separate
application authentication.

## Readiness and observations

Health reports the checked harness identity and executable, adapter capabilities,
supported protocol/harness versions, authentication readiness, and admission.
Capacity fields report `maxActiveJobs=8`, live `activeJobs`, and live
`availableSlots`. `acceptingJobs` describes admission rather than spare capacity.

`nucleus health` prints that document but exits nonzero unless the daemon is
compatible, authenticated, and accepting work. A healthy runtime may still
report `acceptingJobs=false` under quota or a hold.

Iatreion uses `nucleus status-snapshot --json`. It reads health and attempts a
maintenance-detail read for at most 250 ms. It reports daemon readiness,
admission, and capacity without reading account usage, job content, or logs.
Missing maintenance detail remains an explicit incomplete observation.

Live service reads do not prove requester domain success. No service-availability
or general response-latency objective is promised. The always-on macOS service
relies on launchd; a foreground instance does not prove launchd readiness.

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
`maintenance health RUN_ID` checks for the sole matching owner, zero unfinished
jobs and active slots, drained admission guards, authenticated credentials,
supported protocol, and a ready exact harness. It returns the health document
unchanged. The typed
client provides `health_for_deployment`; this exception is solely for
installation readiness, never ordinary requester admission. Only the service
installer holding its own exclusive activity guard may account for that guard
locally; the public health proof requires all guards drained.

Without a hold, deployment readiness also accepts a healthy runtime whose
admission is paused by reported low, exhausted, or unknown quota. Harness,
authentication, protocol, and execution checks still apply. An unexplained
admission pause fails. The installer reads raw health through `service status`
for this proof. Quota policy and ordinary requester admission remain unchanged.

The HTTP surfaces are GET `/v1/maintenance` and POST
`/v1/maintenance/{hold,release}`. Each POST accepts exactly
`{"run_id":"OWNER"}` and returns maintenance status. The typed client owns
these request/response types.

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

## Installation guarantees

Use matching sealed CLI, daemon, installer, and Chancery bundle candidates.
The Rust `nucleus-install` executable is sealed beside the tested CLI and daemon.
Its `install --binary ABS --daemon ABS --codex ABS --bundle ABS` command uses
immutable `cell-install-v2` packages and the Nucleus-owned service installer.
Public CLI and daemon copies remain service-owned so captured prior programs
and schema rollback evidence are preserved. The predecessor format-1 package
remains verifiable. `inspect` and `verify-release ABS` are read-only; they do
not execute a retained installer or restore authentication.

The packaging installer stages the release and matching documentation bundle.
The service installer captures prior public programs, replaces the LaunchAgent,
and allows up to two minutes for migration, compaction, and health. The daemon
stays in the foreground under launchd. A failed cutover restores captured
programs and service configuration only when the database schema is unchanged.
A schema change prevents binary-only rollback. Provider and release selectors
recover with their programs. Authentication is excluded from rollback.

After installation, `nucleus --register-usage` registers the command inventory
without product work. Verify matching programs, the exact harness, account, and
runtime readiness before restoring requester admission. A reported quota pause
can remain after successful installation.

## Coordinated first installation and interrupted cutover

A fresh coordinated installation accepts `codex_bin` and `codex_home` in
Nucleus's deployment settings. Both paths are absolute. `codex_bin` must be the
exact supported Codex version. `codex_home` identifies an existing authenticated
home; settings never contain credential bytes. If Nucleus already owns valid
authentication, installation preserves it. Existing deployments retain a
compatible configured harness and do not import another credential home.
Before CI submission, stage the complete supported Codex runtime with the
candidate installer:

```sh
<TESTED_NUCLEUS_INSTALL> stage-harness --codex /absolute/release/codex
```

The source directory must contain `codex` and its matching
`codex-code-mode-host` from the same release. Staging checks the exact Codex
version and executable files, copies both files, and records their SHA-256
identities in `nucleus-runtime.json`. It publishes the complete directory at
`~/Library/Application Support/Nucleus/harnesses/codex/VERSION/runtime/`.
An identical staged runtime is reused. A different existing directory is refused.
Staging does not select a service runtime, import credentials, or run model work.
The source release is operator-selected; the manifest detects changes to the
selected files and does not independently authenticate their origin.

Installation, health, and admission verify the manifest and required files.
Every selected upgrade requires the staged runtime used by the Nucleus CI gate.
It keeps a configured runtime only when both file identities match that staged
pair; otherwise it selects the staged runtime. An old single-file installation at the
same version also requires this replacement. Deployment captures both file
identities and checks them again before cutover and after installation. Keep
the previous runtime available for supported recovery. Preserve credentials,
retained jobs, and existing failure halts.

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

## Backup, schema recovery, and retention

Nucleus has no automatic backup or restore command. Quiesce requesters and stop
the service before a consistent backup or restoration. A SQLite-aware backup
must preserve the database state; other copy methods must preserve the database
and WAL sidecars as one consistent set. Copying only the live main database is
incomplete. Include private `quota-policy.json` and `quota-state.json` beside the
database. A Nucleus backup does not replace requester product backups.

Credential backup and recovery remain separate. Restore with an operator
present, save current state separately, and use a compatible database and binary
pair. Verify health and retained job/output reads before resuming admission.

Store schema 2 has an explicit version-one cutover. It preserves jobs, attempts,
registrations, cancellation, and terminal state. It discards the old mixed log
and historical answered mailbox rows. A pending call with a nonterminal owning
job and attempt prevents cutover; stale terminal-owner calls are discarded.

The transaction writes the new harness-output ledger and sets
`user_version=1000002`. This marker means compaction is pending. Each restart
retries `VACUUM` and a truncating WAL checkpoint. Only successful completion
publishes `user_version=2` and allows startup to continue. A failed compaction
remains pending and visible. Publishing the final marker can leave one bounded
WAL frame.

Version-one binaries cannot open schema 2. Recovery across the cutover requires
an explicit compatible database and binary pair. Keep credential recovery separate.

The LaunchAgent writes `nucleusd.stdout.log` and `nucleusd.stderr.log` under the
private log directory. Use the host's private-log rotation policy. Nucleus has
no automatic output pruning. Do not limit storage by deleting database rows,
immutable registrations, or individual credential-home files.

`nucleus service restart` terminates the daemon and asks launchd to start it
again. Graceful shutdown requests cancellation; startup marks unfinished
attempts `lost`. Quiesce first when attempts must finish. The requester decides
whether any new attempt is safe.

`nucleus service uninstall` removes installed Nucleus programs and the
LaunchAgent. State and logs remain. Removing retained material requires a
separate decision about the data and its recovery needs. No retention horizon,
database-capacity bound, or general support/deprecation interval is promised.

## Command observation

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors preserve
command results. Registration, a selected program, or a catalog entry does not
establish live service readiness.

## Related contracts

- Read `chancery show nucleus.jobs`.
- Read `chancery show nucleus.invocation`.
- Read `chancery show nucleus.authentication`.
- Read `chancery show nucleus.quota`.
- Read `chancery show nucleus.execution.operate`.
- Read `chancery show nucleus.develop.change`.

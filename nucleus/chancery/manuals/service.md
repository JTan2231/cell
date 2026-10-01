# Service lifecycle and maintenance

Nucleus runs as a current-user macOS service. It owns its private state, socket,
program installation, readiness, and durable maintenance holds. Use this feature
to interpret service health, admission, drain, restart, installation, and recovery.
Use `chancery show nucleus.execution.operate` for ordered installation,
recovery, and service-control procedures. Read `nucleus manual` for shared
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

Treat state, credentials, and logs as private. Database and mailbox
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

## Deployment admission

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

Installation checks the native CLI and daemon signatures under Cell's configured
signing policy. It does not call health or check harness, authentication, or
persistent-state integrity. Use the ordinary diagnostic interfaces for those
observations. Maintenance ownership and drain remain required for replacement.

The HTTP surfaces are GET `/v1/maintenance` and POST
`/v1/maintenance/{hold,release}`. Each POST accepts exactly
`{"run_id":"OWNER"}` and returns maintenance status. The typed client owns
these request/response types.

The macOS deployer accepts `--expected-current absent|releases/HASH` and checks
it under the product update lock before selector mutation. With
`CELL_DEPLOYMENT_RUN_ID`, service install/restart requires the sole drained
hold and retains an exclusive activity guard through replacement. The existing guarded database/credential rollback rules remain.

Daemon startup retires terminal records from the removed built-in deployment
probe. Retirement is limited to requester `nucleus-deployment`, label
`Verify Nucleus deployment`, and the former deterministic job ID derived from
that requester ID. It removes only those jobs, their attempts, and raw output;
children, tool calls, or unfinished work block retirement. Ordinary job history
and shared schemas remain unchanged. This is not a general pruning API.

## Installation guarantees

The Rust `nucleus-install` executable packages the CLI, daemon, installer, and
Chancery bundle. Its `install --binary ABS --daemon ABS --codex ABS --bundle ABS`
command selects a `cell-install-v2` package and invokes the Nucleus service
installer. Public CLI and daemon copies remain service-owned. `inspect` reads
release metadata. There are no installer `verify` or `verify-release` commands.

macOS service installation also requires Cell's persistent signing policy.
The supplied native CLI and daemon must use its exact certificate and the
permanent `nucleus/nucleus` and `nucleus/nucleusd` product/artifact keys.
Their code identifiers are `NAMESPACE.nucleus.nucleus` and
`NAMESPACE.nucleus.nucleusd`, where `NAMESPACE` is the configured namespace.
The service installer verifies input before copying and installed copies before
launchd bootstrap. Missing or invalid policy, unsigned or ad hoc input, a
different certificate, a wrong identifier, or failed verification stops the
installation. It does not choose another identity or re-sign input. Read
`chancery show ci-manager.signing.operate` for policy setup and explicit changes.

The service installer captures prior public programs, replaces the LaunchAgent,
imports a supplied credential source when needed, and starts the daemon under
launchd. It does not wait for health, migration, or compaction. Normal daemon
startup and job admission keep their existing compatibility and readiness checks.
Installation success establishes completed setup, not operational readiness.

A failed file or service operation can restore captured programs and service
configuration only when the database schema is unchanged. Authentication is
excluded from rollback. An uncertain service cutover retains the candidate
package and journal for recovery without comparing program bytes.

After installation, `nucleus --register-usage` registers the command inventory
without product work. Health and account diagnostics remain separate operations.

## Coordinated first installation and interrupted cutover

A fresh coordinated installation accepts `codex_bin` and `codex_home` in
Nucleus's deployment settings. Both paths are absolute. Runtime execution requires the supported Codex version. `codex_home` identifies an existing authenticated
home; settings never contain credential bytes. If Nucleus already owns valid
authentication, installation preserves it. Existing deployments retain a
compatible configured harness and do not import another credential home.
Before CI submission, stage the complete supported Codex runtime with the
candidate installer:

```sh
<TESTED_NUCLEUS_INSTALL> stage-harness --codex /absolute/release/codex
```

The source directory must contain `codex` and `codex-code-mode-host`. Staging
copies both files and records their SHA-256 identities in `nucleus-runtime.json`
for the ordinary runtime checks. It does not compare source versions or verify
the staged files. An existing destination is reused from its recorded manifest.
Staging does not select a service runtime, import credentials, or run model work.

A selected upgrade uses the supported version's staged runtime unless deployment
settings supply `codex_bin`. An affected-only deployment preserves the path from
the installed LaunchAgent. Installation does not compare runtime file identities.
Health and admission keep their ordinary manifest and harness checks.

The installer persists the run's local admission hold before a fresh daemon
exists. It starts the service under that hold so dependent products can finish
configuration. Admission opens only at group release after configuration.
The installer retries the owned release request for up to 120 seconds while
the service socket is unavailable. An API rejection fails immediately. This
release operation does not call health or submit work.

Before selector or service replacement, the installer writes private
`service-cutover.json` with its owner, prior package, candidate and harness.
Recovery uses this journal to select and reinstall the exact candidate through
`nucleus service recover --daemon ABS --codex ABS` with the recorded
`CELL_DEPLOYMENT_RUN_ID`. Recovery reinstalls the recorded candidate and starts its service.
The service must have its sole drained hold. If it is stopped, recovery reads
the database without migration and requires every retained job and attempt to
be terminal. It does not cancel or retry requester work.

Recovery can import the recorded authentication source only if Nucleus's owned
authentication file is absent. It never rolls back a credential or database.
A schema or service failure keeps the candidate and journal for recovery.
Unknown ownership or unfinished jobs keep admission held. Successful recovery removes the cutover journal after service setup completes.

Deployment settings accept only `codex_bin` and `codex_home`, both strings.
Unknown keys or values of another type fail inspection before admission holds.

## Schema compatibility and retention

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

Version-one binaries cannot open schema 2. Select a program that supports the
current schema. Keep credential recovery separate.

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

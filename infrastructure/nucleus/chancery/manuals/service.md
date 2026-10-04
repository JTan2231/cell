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

An explicitly authorized operator owns a durable hold through:

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

Service installation copies the supplied programs and starts the selected
LaunchAgent. It does not audit native signatures or wait for health. Explicit
maintenance operations retain their separate ownership and drain requirements.

The HTTP surfaces are GET `/v1/maintenance` and POST
`/v1/maintenance/{hold,release}`. Each POST accepts exactly
`{"run_id":"OWNER"}` and returns maintenance status. The typed client owns
these request/response types.

The macOS deployer accepts `--expected-current absent|releases/ID` and checks
it under the product update lock before selector mutation. With
`CELL_DEPLOYMENT_RUN_ID`, service install/restart requires the sole drained
hold and retains an exclusive activity guard through replacement. Completed installation effects remain after failure. Authentication moves only forward.

Daemon startup retires terminal records from the removed built-in deployment
probe. Retirement is limited to requester `nucleus-deployment`, label
`Verify Nucleus deployment`, and the former deterministic job ID derived from
that requester ID. It removes only those jobs, their attempts, and raw output;
children, tool calls, or unfinished work block retirement. Ordinary job history
and shared schemas remain unchanged. This is not a general pruning API.

## Installation guarantees

The Rust `nucleus-install` executable packages the CLI, daemon, installer, and
Chancery bundle. Its `install --binary ABS --daemon ABS --codex ABS --bundle ABS`
command selects a `cell-install-v4` package and invokes the Nucleus service
installer. Public CLI and daemon copies remain service-owned regular files at
`~/.local/bin/nucleus` and `~/.local/libexec/nucleusd`. Updates preserve those
actual executable paths. The installer uses the fixed
`~/Library/Application Support/Nucleus/install/runtime/` tree. The provider
directory selector uses the selected retained archive; archive UUIDs identify
retained releases. Each runtime file replacement is atomic; the
complete tree is not one atomic update. `inspect` reads release metadata. There are no installer `verify` or `verify-release` commands.

Packaging signs the native programs under Cell's configured host identity.
The service installer copies the CLI and daemon, replaces the LaunchAgent,
imports a supplied credential source when requested, and starts the daemon under
launchd. It does not audit signatures or wait for health, migration, or compaction.
Normal startup and admission keep their existing harness and authentication rules.

A file or service failure retains completed effects. Installation does not capture
prior program bytes, read the database schema for rollback, or restore a previous
service. Inspect the failed operation and explicitly choose the next installation.
Preserve current credentials and database state.

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
settings supply `codex_bin`. If no staged runtime exists, installation uses the path from the installed
LaunchAgent. Installation does not compare runtime file identities.
Health and admission keep their ordinary manifest and harness checks.

The product manifest invokes `nucleus-install deploy` with a schema-two
installation request. It selects the supplied CLI, daemon, installer, and provider
bundle, then invokes service installation. It does not perform the previous
inspect/hold/drain/apply/configure/release/activate protocol, keep a service-cutover
journal, or automatically recover an unfinished deployment.

Deployment settings accept only `codex_bin` and `codex_home`, both absolute
string paths. Unknown fields or types fail the product command. The executor
reports that exit and retains completed effects without interpreting the output.
Use explicit maintenance commands before installation when admitted work must
finish. Installation can interrupt attempts; requesters own any retry decision.

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

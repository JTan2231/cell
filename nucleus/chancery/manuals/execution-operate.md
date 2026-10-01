# Operate Nucleus agent execution

Use this contract to inspect or operate Nucleus jobs, authentication, and the
current user's service. Nucleus owns runtime execution; requesters own domain
results and retry decisions. A completed job does not establish domain success.

Read `nucleus manual` before shared maintenance. Read
`chancery resolve nucleus.execution.operate` for this procedure and the complete
feature contracts. Use `chancery show ID` for one subject:

| Feature | Detailed contract |
| --- | --- |
| Jobs and attempts | `nucleus.jobs` |
| Invocation fields, permissions, and harness support | `nucleus.invocation` |
| Requester-tool mailbox | `nucleus.requester-tools` |
| Status, output, and retained history | `nucleus.output` |
| Credentials, login, and account reads | `nucleus.authentication` |
| Weekly quota admission | `nucleus.quota` |
| Service readiness, maintenance, and recovery guarantees | `nucleus.service` |

Ordinary work in the current interactive session normally needs no Nucleus
job. Use the requesting product's contract when its domain result or retry
policy is the subject of the task.

## Inspect readiness and work

Start with supported reads:

```sh
nucleus health
nucleus service status
nucleus account --wait 0
nucleus quota
nucleus jobs list --state accepted
nucleus jobs list --state running
nucleus jobs list --state waiting-on-requester
nucleus jobs list --state failed
```

`health` prints the readiness document and exits nonzero unless the daemon is
compatible, authenticated, and accepting work. A reported quota pause can close
admission while runtime status remains healthy. Full execution slots do not
close admission. `authentication_busy` means credential-operation contention;
an active job alone does not imply invalid credentials.

Use `nucleus status-snapshot --json` for the bounded operational report. It reads
health and attempts maintenance detail for at most 250 ms, without reading the
account, job content, or logs. Missing detail stays an incomplete observation.

If the service cannot connect, inspect its status and
`~/Library/Logs/Nucleus/nucleusd.stderr.log`. Do not fall back to an
uncoordinated direct Codex invocation.

## Submit, observe, or cancel one job

Prepare an exact request using `nucleus.invocation`. Submission can consume
account allowance and can cause requester-authorized tool mutations. For
concurrent writable work, assign disjoint directories or worktrees, or have
the requester serialize it. Nucleus does not detect write conflicts.

```sh
nucleus jobs submit <REQUEST_JSON>
nucleus jobs show <JOB_ID>
nucleus jobs status <JOB_ID>
nucleus jobs wait <JOB_ID> --timeout 60
nucleus jobs logs <JOB_ID>
nucleus jobs logs --follow <JOB_ID>
nucleus jobs cancel <JOB_ID>
```

The job ID is the idempotency key. If submission is uncertain, resend only the
same ID and byte-equivalent typed request. Each admitted job has one attempt;
Nucleus never retries automatically. At most eight attempts are active. Further
admitted jobs wait as accepted/pending. Execution timeout starts after slot
acquisition; a requester-tool wait holds the slot through terminal cleanup.

A wait timeout does not cancel a job. Status reads job and mailbox state in
sequence, without an atomic snapshot. Cancellation is idempotent for the exact
job and does not erase history or undo a committed requester mutation.

Inspect the pending call and requester state when an attempt waits on its
requester. Do not invent a response. Inspect domain effects after failure,
timeout, or loss before considering another attempt. Missing structured output
is not authority to rerun work. Successful output decoding and runtime completion
remain separate from requester success.

## Respond to quota deferral

Inspect `nucleus quota`. Preserve the work and its exact request identity on
`quota_deferred`. A rejected new submission creates no job or attempt. Accepted
work can remain pending without a slot or running timeout; started attempts
continue. Scheduled deferral is an expected outcome, not an abend.

Preserve requester deadlines and committed results. Inspect domain effects
before a retry after `quota_exhausted`. Do not edit quota state to simulate
recovery. Quota recovery does not clear maintenance holds, operator pauses, or
Clockwork failure halts. Read `nucleus.quota` for policy, configuration,
observation freshness, and exact protocol behavior.

## Hold and drain for maintenance

1. Quiesce affected requesters and let their admitted multi-job workflows finish.
2. Acquire a durable hold using the deployment run's own ID.
3. Observe actual drain and verify that the sole hold belongs to that run.
4. Perform only the maintenance authorized for the run.
5. Verify the held service before releasing that run's hold.

```sh
nucleus maintenance hold RUN_ID
nucleus maintenance status
nucleus maintenance health RUN_ID
nucleus maintenance release RUN_ID
```

A hold rejects new work but lets admitted work settle. Reads, cancellation,
output, and mailbox responses remain available. Drain includes accepted jobs,
active processes, admission guards, and terminal cleanup. A foreign hold or
unfinished work blocks cutover. Holds survive exit and have no automatic expiry.
Release only the operation's own hold; other holds remain effective.

## Install or update

Select matching tested CLI, daemon, installer, and provider bundle bytes.
Stage the complete supported Codex runtime before installation:

```sh
<TESTED_NUCLEUS_INSTALL> stage-harness --codex /absolute/release/codex
<TESTED_NUCLEUS_INSTALL> install \
  --binary <TESTED_NUCLEUS_BINARY> \
  --daemon <TESTED_NUCLEUS_DAEMON> \
  --bundle <TESTED_NUCLEUS_BUNDLE> \
  --codex <STAGED_CODEX_BINARY>
```

The selected source directory must contain `codex` and the matching
`codex-code-mode-host`. Retain the previous runtime for recovery. Staging copies the pair and records a manifest for normal runtime checks; it does not select a service runtime or import credentials.
The operator selects the source release. File digests do not authenticate its
origin. See `nucleus.invocation` for the exact supported harness and
`nucleus.service` for staging paths and installation guarantees.

For an initial credential import, add
`--codex-home /absolute/signed-in-codex-home`. Treat that home as an import source;
Nucleus owns its resulting private credential. Preserve existing owned
credentials. Never put credential bytes in deployment settings.

Use coordinated maintenance when replacing a daemon could lose work. The
installer starts the service without waiting for health, migration, or compaction.
A failed cutover can restore captured programs only when the database schema
is unchanged. A schema change prevents binary-only rollback. Authentication
is excluded from program and database rollback.

After installation, register command inventory and inspect the selected service:

```sh
nucleus --register-usage
nucleus service status
nucleus health
nucleus account --wait 0
```

Installation performs no artifact-integrity, persistent-state-integrity, or
operational-readiness checks. The commands above are separate diagnostics.
Ordinary daemon startup and admission keep their runtime checks. A held service
still requires the run's sole drained hold. Installation does not reopen quota
admission or submit a synthetic model job.

## Recover interrupted cutover

Inspect the retained run ownership and private `service-cutover.json`. Use its
recorded candidate daemon, harness, and deployment run ID:

```sh
CELL_DEPLOYMENT_RUN_ID=<RECORDED_RUN_ID> \
  nucleus service recover --daemon <RECORDED_DAEMON> --codex <RECORDED_CODEX>
```

Require the sole drained hold. For a stopped service, every retained job and
attempt must be terminal. Unknown ownership or unfinished work keeps admission
held. Recovery selects and starts the recorded candidate; matching files alone
do not prove which executable is resident. It does not cancel or retry work,
roll back a database, or restore an older credential. Authentication import is
allowed only when the owned file is absent and the source was recorded.

Keep the journal and candidate on failure. Recovery completes after the recorded
service setup operation succeeds. Follow the shared manual
for group release; release no unrelated pause or failure halt.

## Recover authentication

Prevent new requester work and let active job and account sessions finish.
Perform attended login and verify the resulting account and service:

```sh
nucleus auth login --device-auth
nucleus account --wait 0
nucleus health
```

Account reads may overlap jobs; attended login is exclusive. Do not copy managed
refresh tokens to requesters or replace credentials because a read is busy.
`annals-usage login --device-auth` delegates to the same operation. Resume only
pauses created for this recovery. Never restore an older `auth.json` as a side
effect of program or database rollback.

## Back up and restore state

Nucleus has no automatic backup or restore command. Select a private destination.

1. Quiesce requesters and wait for jobs to become terminal.
2. Record the Nucleus version, health, and exact Codex executable.
3. Stop the user service:

   ```sh
   launchctl bootout "gui/$(id -u)/org.nucleus.daemon"
   ```

4. Create a SQLite-aware backup of `nucleus.db`. Other copy methods must preserve
   the database and any WAL sidecars as one consistent set.
5. Back up the credential home separately only when credential recovery is required.
6. Include `quota-policy.json` and `quota-state.json` beside the database. Include
   logs, service configuration, and requester state as needed.
7. Start the same service and check readiness:

   ```sh
   launchctl bootstrap "gui/$(id -u)" \
     "$HOME/Library/LaunchAgents/org.nucleus.daemon.plist"
   nucleus health
   ```

A live copy of only the main database is incomplete. A Nucleus backup does not
replace requester backups. Use `nucleus.service` for default paths, retained
state, schema-cutover guarantees, and recovery limits.

Perform restoration with an operator present. Quiesce requesters and stop the
service. Save current state before restoring a compatible database and binary
pair. Version-one binaries cannot open schema 2. Do not bypass pending
compaction or migration failures. Verify health and retained job and output
reads before resuming. Recover credentials through their separate procedure.

## Restart or remove the service

Quiesce first when active attempts must finish. `nucleus service restart`
terminates the daemon; startup marks unfinished attempts lost. The requester
owns recovery. `nucleus service uninstall` removes installed programs and the
LaunchAgent but retains state and logs. Removing retained material needs a
separate decision about that data and its recovery needs.

Monitor private state and logs with `du -sh` at the default paths documented in
`nucleus.service`. Use the host's private-log rotation policy. Nucleus has no
automatic output pruning. Do not delete database rows, immutable registrations,
or individual credential-home files to limit storage.

## Privacy and command usage

Treat state, logs, and backups as private. They can contain complete prompts,
source text, tool arguments and results, exact harness output, and credentials.
Socket ownership and filesystem permissions are the trust boundary; there is no
application-level authentication. Read only the records needed for the task.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery records
command identity, time, and thread ID, not arguments, output, or outcomes.
Internal product calls are excluded. Recording errors preserve command results.

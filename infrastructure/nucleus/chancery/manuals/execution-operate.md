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
Stage the complete supported Codex runtime before CI submission:

```sh
<TESTED_NUCLEUS_INSTALL> stage-harness --codex /absolute/release/codex
```

Submit the selected committed source from the Cell root:

```sh
./ci.sh submit COMMIT --deploy nucleus
```

CI and Telete are the deployment route. Verify the retained manager job outcome.
For explicit setup values, add `--settings /absolute/private/settings.json`
with a canonical `nucleus` object containing `codex_bin` or `codex_home` paths.
Telete freezes these settings with the job. The product setup and explicit
service-recovery API remains:

```sh
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

For an initial credential import, select `codex_home` in the submit settings.
The underlying recovery installer accepts `--codex-home` for that same input.
Treat the selected authenticated home as an import source;
Nucleus owns its resulting private credential. Preserve existing owned
credentials. Never put credential bytes in deployment settings.

Use coordinated maintenance when replacing a daemon could lose work. The
installer starts the service without waiting for health, migration, or compaction.
A failed installation retains completed filesystem and service effects. It does
not capture prior programs or restore a prior service. Preserve current database
state and authentication when selecting the next installation.

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

## Handle an interrupted installation

Inspect the retained manifest executor step, child identity and logs. Wait until
that process has stopped. Its effects are unknown until the affected paths and
service operations are inspected. The executor performs no automatic recovery.

Select the intended program and harness paths explicitly, then run the supported
installation command. Preserve current database state and owned credentials.
Use explicit maintenance when admitted work must finish. A named legacy hold
still requires its exact owner and drain before an explicit service recovery;
release only the hold created for that operation. Acknowledge the executor to
release its admission separately from any product maintenance operation.

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

Treat state and logs as private. They can contain complete prompts,
source text, tool arguments and results, exact harness output, and credentials.
Socket ownership and filesystem permissions are the trust boundary; there is no
application-level authentication. Read only the records needed for the task.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery records
command identity, time, and thread ID, not arguments, output, or outcomes.
Internal product calls are excluded. Recording errors preserve command results.

# Agent jobs

Nucleus admits exact runtime requests and supervises one Codex attempt for each
job. It owns execution state, capacity, cancellation, and retained runtime
history. Requesters own their domain results, duplicate handling, and the
decision to make another attempt. Use this feature to submit, inspect, wait for,
or cancel an exact job. Ordinary work in an interactive agent session does not
require a Nucleus job merely because it uses a model.

## Interfaces

```sh
nucleus health
nucleus jobs submit <REQUEST_JSON>
nucleus jobs list --requester PROGRAM --requester-id REQUESTER_ID
nucleus jobs list --state accepted
nucleus jobs list --state running
nucleus jobs list --state waiting-on-requester
nucleus jobs list --state failed
nucleus jobs show JOB_ID
nucleus jobs status JOB_ID
nucleus jobs wait JOB_ID --timeout 60
nucleus jobs cancel JOB_ID
```

Submission accepts a request file or standard input. The HTTP surfaces are:

```text
POST /v1/jobs
GET  /v1/jobs
GET  /v1/jobs/{job}
POST /v1/jobs/{job}/cancel
```

The supported interfaces use the current-user CLI or version-one HTTP over
Nucleus's filesystem-protected Unix socket. There is no TCP endpoint or
application-level authentication. Reading a Chancery entry grants no access or
authority to submit, cancel, mutate requester data, or control the service.

## Request and identity

One v1 job request contains only runtime information:

```json
{
  "version": 1,
  "id": "explain-unix-socket-01",
  "label": "Explain a Unix socket",
  "requester": {
    "program": "example-client",
    "id": "run-01"
  },
  "instructions": "Answer the user directly, use no tools, and keep the response to one short line.",
  "prompt": "Explain in one sentence what a Unix socket is.",
  "invocation": {
    "version": 1,
    "harness": "codex",
    "model": "gpt-5.6-terra",
    "reasoningEffort": "low",
    "cwd": "/absolute/working/directory",
    "workspaceAccess": "none",
    "builtinTools": {
      "localExecution": false,
      "webSearch": false
    },
    "timeoutSeconds": 90
  }
}
```

The requester chooses `id`, which is the idempotency key. Resubmitting the same
ID and byte-equivalent typed request returns the existing job. Reusing the ID
with a different request digest causes a conflict. The `(requester.program,
requester.id)` index lets reports find every job for one domain run. Nucleus
does not need the domain schema. Optional `parent` names a job for invocation
provenance. It does not define workflow behavior.

The example uses one supported policy. `nucleus.invocation` defines every
invocation field, instruction role, workspace permission, and harness check.
Persist the exact typed request and its domain-run correlation before submission.
If the result of submission is uncertain, repeat only the byte-equivalent
request under the same job ID. A new attempt requires a new ID and a requester
decision that the operation is safe. There is no hidden direct-Codex fallback.

## Admission and capacity

The daemon stores admitted requests and owns eight execution slots. At most
eight attempts can hold live Codex app-server processes at once. Further
admitted jobs remain `accepted` with their attempts `pending` until a slot
opens. Invocation timeout starts only after an attempt owns a slot. A
requester-tool wait keeps the slot because the app-server process is still
live. Cancellation while queued makes the pending attempt `cancelled` without
starting Codex. Admission remains available while all slots are occupied, and
version one does not impose a queue-depth bound or schedule workflow
dependencies.

Admission checks the exact supported harness and invocation semantics before
creating a job. Managed authentication and any applicable quota gate must be
ready. A free execution slot is not required for acceptance. `health` reports
`maxActiveJobs=8`, live `activeJobs`, and live `availableSlots`. Its
`acceptingJobs` field describes admission, not whether a slot is free.

A deployment hold refuses new work with HTTP 503 `deployment_maintenance`.
A quota rejection uses HTTP 429 `quota_deferred` and creates no job or attempt.
Exact replay of an admitted request remains available. Accepted main-Codex jobs
can wait for quota without holding a slot or starting their execution timeout;
started attempts continue. A deployment hold permits admitted work to settle.
Read `nucleus.quota` and `nucleus.service` for those conditions' complete rules.

Submitting can consume account allowance. Nucleus does not compare working
directories or coordinate conflicting mutations. Use disjoint directories or
worktrees for concurrent writes, or serialize those writes in the requester.
The scheduler does not interpret a work-packet graph, priorities, or success rule.

## Observations and terminal state

`jobs status` returns runtime and requester identity, current attempt state and
ID, pending call IDs and names, final-output availability, and terminal reason
and message. It reads mailbox and job state in sequence, without an atomic
snapshot. Initial read errors remain errors. `jobs list` defaults to 20 and
retains its continuation behavior. Reads describe committed local state at the
read boundary; no start or completion latency objective is promised.

`jobs wait --timeout 60` returns one terminal or timeout observation. That wait
timeout does not cancel the job or make the attempt terminal. The invocation's
own positive `timeoutSeconds` is a wall-clock bound that starts after slot
acquisition. A `waiting_on_requester` attempt keeps its slot through terminal
cleanup because its supervised process remains live.

A completed attempt can expose derived `threadId`, `turnId`, and `finalMessage`.
Missing or conflicting startup evidence remains missing output. Failed,
cancelled, timed-out, and lost attempts expose no successful structured output.
Read `nucleus.output` for exact observation and decoding rules. A successful
tool response or completed attempt does not establish application success.

## Cancellation, restart, and recovery

Cancellation records durable intent for one exact job. Repeating it is
idempotent. Intent remains effective when cancellation overlaps startup.
Cancellation does not delete a job or its output and cannot undo a requester
mutation already committed.

Graceful shutdown requests cancellation. Daemon startup marks every unfinished
attempt `lost`, including pending attempts. Nucleus cannot resume the prior
app-server process and never creates a replacement attempt automatically.
After failure, timeout, loss, or quota exhaustion, inspect requester-owned state
before considering new work. Preserve any domain result that already committed.

If a job waits for a requester, inspect its pending mailbox call and the
requester's recoverable state. Do not invent a tool result. If Nucleus cannot
connect, inspect `nucleus service status` and
`~/Library/Logs/Nucleus/nucleusd.stderr.log`.

## Limits and private state

Nucleus private state can contain complete prompts, source content, tool
arguments and results, exact harness output, and terminal diagnostics. Keep
state, logs, and backups private. No automatic output pruning or retention
horizon is promised. Use supported interfaces; direct SQLite integration is
unsupported.

Beyond eight active attempts and one attempt per job, no throughput, queue-depth,
database-capacity, output-volume, service-availability, or deprecation interval
is promised. Nucleus has no workflow engine, project registry, or automatic retry.

## Related contracts

- Read `chancery show nucleus.invocation`.
- Read `chancery show nucleus.requester-tools`.
- Read `chancery show nucleus.output`.
- Read `chancery show nucleus.authentication`.
- Read `chancery show nucleus.quota`.
- Read `chancery show nucleus.service`.
- Read `chancery show nucleus.requester.integrate`.

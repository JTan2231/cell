# Job output and history

Nucleus retains exact non-authentication harness observations and operational job
history. It derives structured final output when retained evidence supports it.
Use this feature to inspect a selected job, follow output, decode historical
observations, or distinguish runtime evidence from a requester-owned result.

## Interfaces

```sh
nucleus jobs show JOB_ID
nucleus jobs status JOB_ID
nucleus jobs logs JOB_ID
nucleus jobs logs --follow JOB_ID
```

```text
GET /v1/jobs?requesterProgram=PROGRAM&requesterId=REQUESTER_ID
GET /v1/jobs/{job}
GET /v1/jobs/{job}/logs?after=0
GET /v1/jobs/{job}/logs?after=0&follow=true
GET /v1/schemas/{schema}
```

The supported interfaces use the current-user CLI or version-one HTTP over
Nucleus's filesystem-protected Unix socket. There is no TCP endpoint or
application-level authentication. Reading a Chancery entry grants no access or
authority to submit, cancel, mutate requester data, or control the service.

Choose the exact requester and job before reading detailed output. A read exposes
retained private material; it does not run another model attempt or validate the
requester's domain result.

## Structured final output

A completed attempt also exposes a small structured `output` object containing
`threadId`, `turnId`, and `finalMessage`. Nucleus derives that object at read
time from the attempt's stdout atoms; it is not another stored result. The
projection identifies the supported invocation's startup sequence from its
retained successful empty MCP-inventory response: response IDs `1/2/3` for
inventory/thread/turn with API-key authentication, or `2/3/4` with managed
authentication. It then binds only the corresponding thread and turn start
responses, accepts only their correlated messages, and freezes at that turn's
terminal notification. Missing or conflicting startup evidence does not produce
structured output. Server requests and error responses cannot establish these
identities. Failed, cancelled, timed-out, and lost attempts expose no successful
structured output.

This decoding uses the retained attempt's evidence rather than the current
authentication configuration. A decoder repair can therefore recover structured
output when an existing completed job is read again, without rewriting its raw
atoms, changing its terminal state, or running another attempt. It cannot recover
records that were not retained or decide whether requester-owned work should be
retried. Outgoing requests and authentication responses remain excluded from the
ledger.

## Exact observations and read envelopes

SQLite stores operational authority separately from reporting observations.
Jobs, attempts, cancellation, immutable registrations, and the dynamic-tool
mailbox are operational records. Each retained reporting observation contains the attempt identity, arrival
sequence, Nucleus observation time, and raw stdout payload bytes. There is one row for every non-authentication `FromHarness`
JSONL record, with only its line delimiter removed. A response to a
host-managed authentication request is consumed in memory but deliberately not
emitted or stored, and managed-worker stderr is drained without retention.
Harness input, lifecycle/control events, other stderr chunks, requester
results, schema IDs, digests, event types, token totals, and final-output fields
are not separately retained reporting observations.

`GET /logs` retains the version-one compatibility envelope, but its surrounding
fields are calculated from the output atom and owning attempt:

```json
{
  "version": 1,
  "jobId": "todo-research-2026-08-26-01",
  "attemptId": "attempt_0198...",
  "sequence": 12,
  "observedAt": "2026-08-26T18:31:39.441Z",
  "stream": "harness.output",
  "schemaId": "codex.app-server.protocol.0.154.0-alpha.6.2",
  "payload": {"jsonrpc":"2.0", "method":"turn/started", "params":{"turn":{"id":"..."}}},
  "payloadDigest": "sha256:..."
}
```

`jobId`, `stream=harness.output`, and the Codex protocol `schemaId` come from the
owning attempt; `payloadDigest` is calculated when read. A JSON value is exposed
directly only when the public raw-value representation is byte-identical to the
stored payload. Malformed or non-UTF-8 output, and valid JSON with surrounding
whitespace that the raw-value type would strip, remains byte-exact in SQLite and
is exposed reversibly through `nucleus.raw-bytes.v1`'s base64 envelope. The
public digest always covers the public payload bytes. Sequence is per attempt.
Version one admits exactly one attempt per job, so the existing numeric job-log
cursor is unambiguous.

The schema registry retains the exact generated Codex JSON Schema bundle for
decoder discovery and immutable request/tool registrations. Output rows do not
duplicate schema identity. Read-time or requester-owned pipelines interpret
methods, messages, usage observations, totals, coverage, and prices.

Job and attempt state, timestamps, cancellation, and terminal fields record the
execution lifecycle. A daemon restart marks unfinished attempts `lost` without
adding a reporting row. Nucleus never stores stderr chunks. It retains a
bounded tail in memory and adds sanitized text to `terminalMessage` on failure.
The complete stored message includes the underlying failure, has control
characters sanitized, and is capped at 16 KiB.

Reporting reads:

1. `GET /v1/jobs?requesterProgram=annals&requesterId=<model-run-token>`
2. `GET /v1/jobs/<id>/logs?after=0`
3. `GET /v1/schemas/<schema-id>` for a generated harness decoder it does not
   have cached

`follow=true` is a bounded long poll returning one output-only `LogsResponseV1`
page. A CLI or UI repeats it. Reports calculate projections from these atoms
and operational attribution; Nucleus does not store reporting materializations.

## Status, limits, and recovery

`jobs status` reports runtime and requester identity, current attempt state and
ID, pending call IDs and names, final-output availability, and terminal reason
and message. It reads mailbox and job state in sequence, without an atomic
snapshot. Initial read errors remain errors. `jobs list` defaults to 20 and
retains its continuation behavior. `jobs wait --timeout 60` returns one terminal
or timeout observation; that wait timeout does not cancel the job.

Nucleus is authoritative for recorded runtime observations. Requesters remain
authoritative for their own records and success. Read-time reporting pipelines
own interpretations, totals, coverage, and prices. Missing observations cannot
be reconstructed from today's account mode or treated as zero usage. No
output-arrival, observation-completeness beyond received non-authentication records,
retention-horizon, or billing promise is added here.

Nucleus private state can contain complete prompts, source content, tool
arguments and results, exact harness output, and terminal diagnostics. Keep
state, logs, and backups private. No automatic output pruning or retention
horizon is promised. Use supported interfaces; direct SQLite integration is
unsupported.

No public direct-SQL integration, stderr stream, stored reporting projection,
automatic output pruning, or replacement attempt is supported. A decoder repair
can change derived output from retained evidence without changing the terminal
operational record. It does not authorize requester retries.

## Related contracts

- Read `chancery show nucleus.jobs`.
- Read `chancery show nucleus.requester-tools`.
- Read `chancery show nucleus.authentication`.
- Read `chancery show nucleus.service`.

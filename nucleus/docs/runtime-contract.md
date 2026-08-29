# Runtime contract

## What a requester says

One v1 job request contains only runtime information:

```json
{
  "version": 1,
  "id": "nucleus-smoke-01",
  "label": "Verify Nucleus agent execution",
  "requester": {
    "program": "nucleus-smoke",
    "id": "run-01"
  },
  "instructions": "Answer the user directly, use no tools, and keep the response to one short line.",
  "prompt": "Reply with exactly NUCLEUS_SMOKE_OK.",
  "invocation": {
    "version": 1,
    "harness": "codex",
    "model": "gpt-5.6-terra",
    "reasoningEffort": "low",
    "cwd": "/Users/joey/rust/cell/nucleus",
    "workspaceAccess": "none",
    "builtinTools": {
      "localExecution": false,
      "webSearch": false
    },
    "timeoutSeconds": 90
  }
}
```

`id` is chosen by the requester and is the idempotency key. Resubmitting the
same ID and byte-equivalent typed request returns the existing job. Reusing the
ID with a different request digest is a conflict. `(requester.program,
requester.id)` is indexed so a reporting surface can find every job for one
domain run without Nucleus knowing that domain's schema. `parent` is an optional
job ID for invocation provenance; it does not create workflow semantics.

`instructions` carries the requester's base contract and optional
`developerInstructions` carries its distinct developer contract; `prompt` is
the input for this particular job. The Codex adapter forwards the three values
separately as `baseInstructions`, `developerInstructions`, and the turn's user
text. It clears bundled model messages. Existing Annals and Todo instruction
priority is therefore preserved rather than flattened into one message.

The configurable invocation domain is deliberately closed:

- exact harness and model
- optional reasoning effort (`low`, `medium`, `high`, or `max`)
- absolute working directory
- workspace access (`none`, `read-only`, or `read-write`)
- explicit built-in tool policy (`localExecution` and `webSearch`)
- positive wall-clock timeout
- optional versioned toolset reference
- optional ID of a short-lived launch context registered immediately before
  submission

Every v1 invocation is ephemeral, unattended, enables Codex raw-response
telemetry, and uses `approvalPolicy=never`. There is one attempt and no
automatic retry. There is no request field for a command, argv, Codex config,
approval behavior, isolation mode, or output format.

A requester that must preserve caller-environment behavior can use
`POST /v1/launch-contexts`. The body contains the requester identity and a
complete environment snapshot; the response contains a 120-second, single-use
ID. Nucleus retains the values only in daemon memory. A fresh job with that ID
starts Codex with an empty environment, applies the snapshot, removes
`CODEX_EXEC_SERVER_URL`, and replaces `CODEX_HOME` with the Nucleus-owned
isolated home. The stored job contains only the opaque ID. An identical
resubmission finds the existing job before checking or consuming the one-shot
context. Todo's current stages deliberately do not register a launch context.

## How harness differences are handled

An adapter translates the stable domain to one harness. Before accepting a job,
the Codex adapter inspects the exact executable, reads its version and bundled
model catalog, and generates its app-server protocol schema. It then checks each
requested semantic. For example, it rejects a model missing from that installed
catalog, an unsupported reasoning effort, a missing working directory, or a
harness other than `codex`.

The v1 adapter is explicitly bound to Codex `0.146.0`. It rejects any other
version and verifies the generated schema still contains every protocol method,
field, and enum value Nucleus consumes before it creates a job row. Supporting a
new Codex release is therefore an adapter change with tests, not an optimistic
version-range match.

The job records both harness and adapter versions. Adding another harness means
adding another adapter that proves it can implement the same v1 meanings; it
does not mean adding that harness's settings to the public request. A genuinely
new portable semantic requires a new version of the Nucleus invocation
contract.

`workspaceAccess=none` gives Codex an empty temporary working directory under a
read-only sandbox and explicitly sends `environments: []` on both thread and
turn start. `read-only` uses the requested directory under a read-only sandbox.
`read-write` uses it under Codex's workspace-write sandbox. Approvals remain
disabled in all three cases. `localExecution=false` removes Codex's
command, inspection, and edit primitives; `webSearch=false` removes live search.
The Codex adapter rejects local execution with `workspaceAccess=none` because it
cannot prove that combination's filesystem semantics. Todo's current
concern-routing, situation-assessment, and design-reconciliation stages use
`workspaceAccess=none`, `localExecution=false`, and `webSearch=false`; Annals
also uses `false/false` and receives only its dynamic tools. The historical
Todo `create_todo` fixture used `true/true` with read-only workspace access.

Current complete requests are checked in at
[`examples/job.smoke.json`](../examples/job.smoke.json) and
[`examples/job.annals.json`](../examples/job.annals.json). The checked-in
[`examples/job.todo.json`](../examples/job.todo.json) and matching schema and
toolset are immutable historical compatibility fixtures, not the current Todo
requester flow.

## Requester-owned tools

A requester registers a versioned toolset before submitting jobs that reference
it. The registration document is immutable by `(provider, name, version)`.

The example below is Todo's immutable historical `create_todo` fixture. Current
Todo uses `todo/concern-routing/1`, `todo/situation-assessment/1`, and
`todo/design-reconciliation/1` with the closed invocation policy above; the
mailbox mechanics are the same.

```json
{
  "version": 1,
  "toolset": {
    "provider": "todo",
    "name": "research-liaison",
    "version": 1
  },
  "definitionsSchemaId": "nucleus.toolset-definitions.v1",
  "definitions": {
    "version": 1,
    "tools": [
      {
        "name": "create_todo",
        "description": "Durably create the one researched todo for this session. Call exactly once, after research is complete. The host supplies provenance, status, and timestamps.",
        "inputSchemaId": "todo.create-todo.arguments.v1",
        "inputSchema": {
          "type": "object",
          "additionalProperties": false,
          "required": ["title", "note"],
          "properties": {
            "title": {"type": "string", "minLength": 1},
            "note": {"type": "string", "minLength": 1}
          }
        }
      }
    ]
  },
  "digest": "sha256:..."
}
```

Nucleus supplies those definitions to Codex as dynamic client tools. When the
model calls one, Nucleus stores the raw arguments and exposes a durable pending
call:

```http
GET /v1/jobs/todo-research-2026-08-26-01/tool-calls?after=0&waitSeconds=30
```

```json
{
  "version": 1,
  "jobId": "todo-research-2026-08-26-01",
  "calls": [{
    "version": 1,
    "state": "pending",
    "createdAt": "2026-08-26T18:31:42.019Z",
    "call": {
      "version": 1,
      "id": "call_Bp91",
      "jobId": "todo-research-2026-08-26-01",
      "attemptId": "attempt_0198...",
      "requestSequence": 18,
      "toolName": "create_todo",
      "argumentsSchemaId": "todo.create-todo.arguments.v1",
      "arguments": {"title":"Centralize agent invocation", "note":"..."}
    }
  }],
  "nextSequence": 18
}
```

A compatible historical Todo requester executes `create_todo` against its own
database, then posts the result. `source` and `direction` are deliberately
absent from the model's arguments; that requester binds both from its
originating request:

```json
{
  "version": 1,
  "callId": "call_Bp91",
  "requester": {"program":"todo", "id":"todo-request-8f53d6"},
  "resultSchemaId": "todo.create-todo.result.v1",
  "result": {"created":true, "todo":{"id":"t19", "title":"Centralize agent invocation"}},
  "isError": false
}
```

Nucleus verifies that the requester identity matches the job, accepts exactly
one result, records it in the operational mailbox, and returns it to the blocked
app-server call. The exact stdout `item/tool/call` record and its pending
mailbox projection are committed in one SQLite transaction, and
`requestSequence` names that output atom. The requester result is not copied
into reporting storage. Its mailbox update is committed before Codex is woken,
and that transaction rejects a new answer once either the owning job or attempt
is terminal. If the requester disappears, the job remains visibly
`waiting_on_requester` until it is cancelled or times out. Nucleus never runs
domain tools itself.

A completed attempt also exposes a small structured `output` object containing
`threadId`, `turnId`, and `finalMessage`. Nucleus derives that object at read
time from the attempt's stdout atoms; it is not another stored result. The
projection binds the active thread and turn start responses, accepts only their
fixed JSON-RPC response identities, accepts only their correlated messages, and
freezes at that turn's terminal notification.

## Harness-output observation ledger

SQLite stores operational authority separately from reporting observations.
Jobs, attempts, cancellation, immutable registrations, and the dynamic-tool
mailbox are operational records. The reporting ledger has exactly four stored
columns: `attempt_id`, arrival `sequence`, Nucleus `observed_at`, and the raw
stdout `payload` bytes. There is one row for every `FromHarness` JSONL record,
with only its line delimiter removed. Harness input, lifecycle/control events,
stderr chunks, requester results, schema IDs, digests, event types, token
totals, and final-output fields are not reporting rows or columns.

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
  "schemaId": "codex.app-server.protocol.0.146.0",
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

The schema registry still retains the exact generated Codex JSON Schema bundle
for decoder discovery and immutable request/tool registrations, but no schema
identity is duplicated on each output row. All interpretations—methods,
messages, usage observations, totals, coverage, and prices—belong to read-time
or requester-owned pipelines.

Lifecycle truth comes from job and attempt state, timestamps, cancellation, and
terminal fields. A daemon restart marks unfinished attempts `lost` without
adding a reporting row. Stderr is never persisted as chunks; a run retains only
a bounded in-memory tail and adds its sanitized text to `terminalMessage` on
failure. The complete stored `terminalMessage`, including the underlying
failure, is control-sanitized and capped at 16 KiB. Cancellation remains durable
at admission boundaries: the daemon
seeds each new invocation's watch from `cancellation_requested_at` after
publishing its sender, so a request overlapping startup cannot be lost.

Reporting reads:

1. `GET /v1/jobs?requesterProgram=annals&requesterId=<model-run-token>`
2. `GET /v1/jobs/<id>/logs?after=0`
3. `GET /v1/schemas/<schema-id>` for a generated harness decoder it does not
   have cached

`follow=true` is a bounded long poll returning one output-only `LogsResponseV1`
page. A CLI or UI repeats it. Reports calculate projections from these atoms
and operational attribution; Nucleus does not store reporting materializations.

## HTTP surface

The v1 server is available only on a per-user Unix socket:

```text
GET    /v1/health
GET    /v1/account?includeUsage=false&waitSeconds=0
POST   /v1/launch-contexts
POST   /v1/jobs
GET    /v1/jobs
GET    /v1/jobs/{job}
POST   /v1/jobs/{job}/cancel
GET    /v1/jobs/{job}/logs
GET    /v1/jobs/{job}/tool-calls
POST   /v1/jobs/{job}/tool-calls/{call}/result
POST   /v1/schemas
GET    /v1/schemas/{schema}
POST   /v1/toolsets
GET    /v1/toolsets/{provider}/{name}/{version}
```

Health reports whether jobs are currently accepted, the checked harness
identity and executable, adapter capabilities, supported protocol/harness
versions, and authentication readiness. `nucleus health` exits nonzero for a
degraded document. Account reads run under the same exclusive credential lease
as jobs: `waitSeconds=0` is a nonblocking try-lock for interactive budget/doctor
commands, while the Annals inbox preflight uses up to 30 seconds. Lease
contention returns `authentication_busy`; credential or account failure returns
`model_auth_unavailable`. `rateLimits` and optional `usage` are the unmodified
results of Codex's `account/rateLimits/read` and `account/usage/read` methods.

## Authentication ownership

The macOS service has one authoritative home at
`~/Library/Application Support/Nucleus/codex-home` (directory mode `0700`). Its
`config.toml` is a private regular file no larger than 64 KiB and may contain
only `cli_auth_credentials_store = "file"`; `auth.json` is a private regular
file with mode `0600`. `nucleus service install --codex-home SOURCE` imports the
currently signed-in `auth.json` after the old daemon has been stopped, so the
import cannot race an in-flight refresh. Credential state is forward-only and
is deliberately excluded from installation rollback: restoring binaries and
the LaunchAgent must never replace a token refreshed by either the old or the
replacement daemon with an earlier, already-consumed credential.

Jobs use isolated temporary Codex homes, but the exclusive credential lease is
held from copy-in through atomic, fsynced refresh copy-back. Account reads and
`nucleus auth login --device-auth` use the same lease. Once imported, Annals and
Todo do not read, write, refresh, or lock Codex credentials themselves.

The standard service installer secures its state directory as mode `0700`; the
daemon secures the database and socket as mode `0600`. There is no TCP listener
and no Nucleus authentication protocol in v1. Local user filesystem permissions
are the trust boundary.

Opening a store-schema-version-1 database performs the explicit version-2
cutover described in the operator manual. Old mixed logs and historical
answered mailbox rows are discarded; a pending requester call blocks migration
only while its owning job and attempt remain nonterminal.
The post-commit compaction can make first start slower, so install and restart
wait up to two minutes for health. Once the schema changes, installer rollback
refuses to restore an incompatible version-one daemon without its matching
database.

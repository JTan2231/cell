# Requester tools

Nucleus transports model tool calls through a durable mailbox. The requester
validates and commits each domain mutation. Use this feature when an application
must expose a constrained toolset and service its calls across requester restarts.
Nucleus never executes a requester domain tool or decides its domain success.

## Interfaces

```text
POST /v1/schemas
GET  /v1/schemas/{schema}
POST /v1/toolsets
GET  /v1/toolsets/{provider}/{name}/{version}
GET  /v1/jobs/{job}/tool-calls?after=0&waitSeconds=30
POST /v1/jobs/{job}/tool-calls/{call}/result
```

The supported interfaces use the current-user CLI or version-one HTTP over
Nucleus's filesystem-protected Unix socket. There is no TCP endpoint or
application-level authentication. Reading a Chancery entry grants no access or
authority to submit, cancel, mutate requester data, or control the service.

Register the exact immutable schemas and toolset before admitting a job that
uses them. Rust requesters use the supported typed client; other languages can
use the documented HTTP protocol. Do not use the human CLI as an application
transport when the client or HTTP interface is available.

## Registration, calls, and results

A requester registers a versioned toolset before submitting jobs that reference
it. The registration document is immutable by `(provider, name, version)`.

The inline example below is Todo's immutable historical `create_todo` fixture.
It illustrates the protocol and does not describe the current Todo workflow.

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

Nucleus checks that the requester identity matches the job. It accepts exactly
one result, records it in the operational mailbox, and returns it to the
blocked app-server call. One SQLite transaction commits the exact stdout
`item/tool/call` record and its pending mailbox projection. `requestSequence`
names that output atom. The requester result is not copied into reporting
storage. Nucleus commits the mailbox update before waking Codex. That
transaction rejects a new answer if the owning job or attempt is terminal.
If the requester disappears, the job remains visibly
`waiting_on_requester` until it is cancelled or times out. Nucleus never runs
domain tools itself.

## Identity and recovery

Schema IDs and the toolset tuple `(provider, name, version)` bind immutable
meaning and digest. An incompatible meaning requires a new identity/version;
retain decoders for historical jobs. Never rewrite an old registration.

The job's requester program and ID identify the domain run. A call ID identifies
one call; `requestSequence` identifies the corresponding per-attempt output atom.
`createdAt` is the retained creation timestamp. The example `after` cursor and
`nextSequence` support bounded mailbox reads. A read reports committed local
state, not an atomic snapshot with a separate job read.

Persist correlation and the exact result around the domain commit. An identical
result can be replayed; a conflicting result cannot replace the accepted one.
A requester restart can rediscover a durable pending call and its own committed
result. It must avoid repeating a domain mutation when posting a result again.
A requester commit remains authoritative if runtime transport later fails.

An unanswered call keeps the job visibly `waiting_on_requester` and occupies its
execution slot until cancellation, timeout, or terminal cleanup. A Nucleus
restart marks unfinished attempts lost and cannot resume the harness process.
Inspect domain state before the requester authorizes a new job. Neither a
mailbox response nor Nucleus completion proves application success.

## Limits and private state

Nucleus private state can contain complete prompts, source content, tool
arguments and results, exact harness output, and terminal diagnostics. Keep
state and logs private. No automatic output pruning or retention
horizon is promised. Use supported interfaces; direct SQLite integration is
unsupported.

Nucleus has no automatic requester-tool execution, invented result, automatic
replacement attempt, or domain retry policy. No mailbox response-latency,
database-capacity, output-volume, or general retention bound is promised.

## Related contracts

- Read `chancery show nucleus.jobs`.
- Read `chancery show nucleus.invocation`.
- Read `chancery show nucleus.output`.
- Read `chancery show nucleus.requester.integrate`.

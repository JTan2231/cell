# Runtime contract

## What a requester says

One v1 job request contains only runtime information:

```json
{
  "version": 1,
  "id": "todo-research-2026-08-26-01",
  "label": "Research centralized agent execution",
  "requester": {
    "program": "todo",
    "id": "todo-request-8f53d6"
  },
  "instructions": "Act as Todo's research liaison. Research without modifying state and create exactly one todo through create_todo.",
  "prompt": "Research the direction thoroughly and create exactly one todo using the supplied tool.",
  "invocation": {
    "version": 1,
    "harness": "codex",
    "model": "gpt-5.6-terra",
    "reasoningEffort": "medium",
    "cwd": "/Users/joey/rust/todo",
    "workspaceAccess": "read-only",
    "builtinTools": {
      "localExecution": true,
      "webSearch": true
    },
    "timeoutSeconds": 3600,
    "toolset": {
      "provider": "todo",
      "name": "research-liaison",
      "version": 1
    }
  }
}
```

`id` is chosen by the requester and is the idempotency key. Resubmitting the
same ID and byte-equivalent typed request returns the existing job. Reusing the
ID with a different request digest is a conflict. `(requester.program,
requester.id)` is indexed so a reporting surface can find every job for one
domain run without Nucleus knowing that domain's schema. `parent` is an optional
job ID for invocation provenance; it does not create workflow semantics.

`instructions` carries the requester's durable liaison contract at instruction
priority; `prompt` is the input for this particular job. The Codex adapter uses
the former as `baseInstructions`, clears bundled model messages, and sends only
the latter as the turn's user text. Annals and Todo can therefore preserve their
current tool and completion contracts without flattening them into a user
message.

The configurable invocation domain is deliberately closed:

- exact harness and model
- optional reasoning effort (`low`, `medium`, `high`, or `max`)
- absolute working directory
- workspace access (`none`, `read-only`, or `read-write`)
- explicit built-in tool policy (`localExecution` and `webSearch`)
- positive wall-clock timeout
- optional versioned toolset reference

Every v1 invocation is ephemeral, unattended, and uses `approvalPolicy=never`.
There is one attempt and no automatic retry. There is no request field for a
command, argv, environment, Codex config, approval behavior, isolation mode, or
output format.

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
read-only sandbox. `read-only` uses the requested directory under a read-only
sandbox. `read-write` uses it under Codex's workspace-write sandbox. Approvals
remain disabled in all three cases. `localExecution=false` removes Codex's
command, inspection, and edit primitives; `webSearch=false` removes live search.
The Codex adapter rejects local execution with `workspaceAccess=none` because it
cannot prove that combination's filesystem semantics. Todo uses `true/true`;
Annals uses `false/false` and receives only its dynamic tools.

The contrasting complete requests are checked in at
[`examples/job.todo.json`](../examples/job.todo.json) and
[`examples/job.annals.json`](../examples/job.annals.json).

## Requester-owned tools

A requester registers a versioned toolset before submitting jobs that reference
it. The registration document is immutable by `(provider, name, version)`.

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

Todo executes `create_todo` against its own database, then posts the result.
`source` and `direction` are deliberately absent from the model's arguments;
Todo binds both from its originating request:

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
one result, records it, and returns it to the blocked app-server call. If the
requester disappears, the job remains visibly `waiting_on_requester` until it is
cancelled or times out. Nucleus never runs domain tools itself.

## Raw log model

SQLite has relational projections only for coordination: jobs, attempts,
registered schemas and toolsets, pending tool calls, and monotonically ordered
log records. It does not have columns for Codex events, token usage, model
messages, or requester tool arguments.

Each external JSONL value is stored as raw JSON and references an immutable
schema registry row:

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

The schema row contains the exact JSON Schema bundle generated by the installed
Codex executable, with producer and version metadata and its own digest. Nucleus
stores that document opaquely. It neither expands the external schema into SQL
columns nor makes job correctness depend on successfully decoding future Codex
fields. Harness input is retained too, making the protocol exchange auditable;
stderr bytes are wrapped in a Nucleus-owned JSON envelope.

Nucleus lifecycle and control events use a small Nucleus-owned schema in the
same ordered log. This covers admission, process start, tool waiting,
cancellation, completion, timeout, and a `lost` attempt detected after daemon
restart. Operational truth therefore does not require inspecting a process tree
or an ephemeral stderr tail.

Reporting reads:

1. `GET /v1/jobs?requesterProgram=annals&requesterId=<model-run-token>`
2. `GET /v1/jobs/<id>/logs?after=0`
3. `GET /v1/schemas/<schema-id>` for any decoder it does not have cached

`follow=true` is a bounded long poll returning one `LogsResponseV1` page. A CLI
or UI repeats it. Reports can add materialized views in their own process; those
views are disposable derivations, not Nucleus's source of truth.

## HTTP surface

The v1 server is available only on a per-user Unix socket:

```text
GET    /v1/health
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

The standard service installer secures its state directory as mode `0700`; the
daemon secures the database and socket as mode `0600`. There is no TCP listener
and no Nucleus authentication protocol in v1. Local user filesystem permissions
are the trust boundary.

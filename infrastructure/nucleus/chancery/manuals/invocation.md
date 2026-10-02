# Invocation policy

Nucleus accepts a closed execution policy for each job. Use this feature to
choose a supported harness, model, instruction set, working directory, tools,
and timeout. It defines what an accepted execution is allowed to use; requester
policy still decides which permissions a particular domain task needs.

## Interfaces and a policy example

An invocation is part of the version-one request submitted through
`POST /v1/jobs` or `nucleus jobs submit <REQUEST_JSON>`. Inspect the installed
capabilities with `nucleus health` or `GET /v1/health`. A launch context is
registered through `POST /v1/launch-contexts`.

```json
{
  "version": 1,
  "harness": "codex",
  "model": "gpt-5.6-terra",
  "reasoningEffort": "low",
  "cwd": "/absolute/working/directory",
  "workspaceAccess": "read-only",
  "builtinTools": {"localExecution": false, "webSearch": false},
  "timeoutSeconds": 90
}
```

The directory must exist and the selected model must be in the exact installed
harness catalog. Read `nucleus.jobs` for a complete enclosing request and its
submission identity.

The supported interfaces use the current-user CLI or version-one HTTP over
Nucleus's filesystem-protected Unix socket. There is no TCP endpoint or
application-level authentication. Reading a Chancery entry grants no access or
authority to submit, cancel, mutate requester data, or control the service.

## Instructions and accepted settings

`instructions` carries the requester's base contract. Optional
`developerInstructions` carries its distinct developer contract. `prompt`
contains this job's input. The Codex adapter forwards the three values
separately as `baseInstructions`, `developerInstructions`, and the turn's user
text. It clears bundled model messages and preserves Annals and Todo
instruction priority.

The invocation accepts only these settings:

- exact harness and model
- optional reasoning effort (`low`, `medium`, `high`, or `max`)
- absolute working directory
- workspace access (`none`, `read-only`, `read-write`, or `unrestricted`)
- explicit built-in tool policy (`localExecution` and `webSearch`)
- positive wall-clock timeout
- optional versioned toolset reference
- optional ID of a short-lived launch context registered immediately before
  submission

The job and HTTP envelopes remain version one. Invocation policy version one
retains `none`, `read-only`, and `read-write`. Policy version two adds
`unrestricted` and accepts the earlier modes unchanged. The typed constructor
selects version two for `unrestricted`; version one rejects that mode. Requesters
must require the `workspace-unrestricted` harness capability before submitting
it. Deploy daemon support before an unrestricted requester. Retain a compatible
daemon when retained jobs contain version-two policies; old binaries cannot
decode the new mode.

Every supported invocation is ephemeral, unattended, enables Codex raw-response
telemetry, and uses `approvalPolicy=never`. There is one attempt and no
automatic retry. There is no request field for a command, argv, Codex config,
approval behavior, isolation mode, or output format.

## Launch context

A requester that must preserve the caller's environment can use
`POST /v1/launch-contexts`. The body contains the requester identity and a
complete environment snapshot. The response contains a single-use ID valid
for 120 seconds. Nucleus retains the values only in daemon memory. A new job with that ID
starts Codex with an empty environment, applies the snapshot, removes
`CODEX_EXEC_SERVER_URL`, and replaces `CODEX_HOME` with the Nucleus-owned
isolated home. The stored job contains only the opaque ID. An identical
resubmission finds the existing job before checking or consuming the one-shot
context.

## Harness and access semantics

An adapter translates the stable domain to one harness. Before accepting a job,
the Codex adapter verifies the complete runtime manifest and required executable
files, then inspects the exact executable, reads its version and bundled
model catalog, and generates its app-server protocol schema. It then checks each
requested semantic. For example, it rejects a model missing from that installed
catalog, an unsupported reasoning effort, a missing working directory, or a
harness other than `codex`.

The v1 adapter requires Codex `0.154.0-alpha.6.2` and rejects other versions. Before it
creates a job row, it checks the generated schema for every protocol method,
field, and enum value Nucleus consumes. Supporting a new Codex release requires
an adapter change and tests. A version-range match is not sufficient.

The job records both harness and adapter versions. A new harness requires an
adapter that implements the same v1 meanings. It does not add harness-specific
settings to the public request. New portable semantics require a new Nucleus
invocation contract version.

`workspaceAccess=none` gives Codex an empty temporary working directory under a
read-only sandbox and explicitly sends `environments: []` on both thread and
turn start. `read-only` uses the requested directory under a read-only sandbox.
`read-write` uses it under Codex's workspace-write sandbox. `unrestricted` uses
the requested directory with Codex's `danger-full-access` mode: filesystem,
process, local socket, and network access are not restricted by the Codex
sandbox. The current user's operating-system permissions still apply.
Approvals remain disabled in all modes. `localExecution=false` removes Codex's
command, inspection, and edit primitives; `webSearch=false` removes live search.
Nucleus does not lock or compare working directories. A requester that submits
concurrent `read-write` or `unrestricted` jobs must give them disjoint working
directories or worktrees, or serialize them itself; the eight-slot scheduler does not resolve
filesystem or external-mutation conflicts.
The Codex adapter rejects local execution with `workspaceAccess=none` because it
cannot prove that combination's filesystem semantics.

The complete supported Codex runtime contains `codex`, its matching
`codex-code-mode-host`, and `nucleus-runtime.json` with both SHA-256 identities.
Installation, health, and admission reject missing, nonexecutable, or changed
files. The source release is operator-selected; the manifest detects changes
and does not independently authenticate origin. Read `nucleus.service` for
staging, selection, and recovery guarantees.

## Effects, failures, and compatibility

A submitted policy can expose the selected workspace or caller environment to
Codex and consume account allowance. Built-in web search and local execution
are separate choices. Nucleus does not infer authority from catalog discovery.
The requester must isolate or serialize writes that can conflict.

Unsupported harnesses, missing models, unsupported reasoning effort, missing
working directories, and unsupported workspace/tool combinations fail admission.
Correct the exact request deliberately. Do not change a retained request under
the same job ID or substitute a direct-Codex fallback.

Invocation policy, job/HTTP protocol, adapter, exact harness, provider release,
and capability contract have independent versions. Deploy accepting daemon
support before a requester emits a new form. Retain compatible decoding for
stored requests. No general harness-upgrade cadence, model-support window,
requester-support window, or deprecation interval is promised.

Nucleus private state can contain complete prompts, source content, tool
arguments and results, exact harness output, and terminal diagnostics. Keep
state and logs private. No automatic output pruning or retention
horizon is promised. Use supported interfaces; direct SQLite integration is
unsupported.

## Related contracts

- Read `chancery show nucleus.jobs`.
- Read `chancery show nucleus.requester-tools`.
- Read `chancery show nucleus.authentication`.
- Read `chancery show nucleus.service`.
- Read `chancery show nucleus.develop.change`.

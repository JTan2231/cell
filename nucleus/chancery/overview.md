# Nucleus

Nucleus runs local agent jobs for the current user. It owns admission,
execution capacity, one supervised attempt per job, authentication, the
requester-tool mailbox, and retained output. Each requesting product owns its
data, validates mutations, decides whether its work succeeded, and decides
whether another attempt is safe.

Read one feature with `chancery show ID`. Read an operation and its required
feature contracts with `chancery resolve ID`. These commands read the installed
publication. They do not contact Nucleus, establish readiness, or authorize work.

## Features

| ID | Read this to understand |
| --- | --- |
| `nucleus.jobs` | Request identity, admission, execution capacity, cancellation, timeouts, and terminal states. |
| `nucleus.invocation` | Request fields, instruction roles, harness support, workspace access, built-in tools, and launch context. |
| `nucleus.requester-tools` | Immutable schemas and toolsets, durable calls and results, and requester mutation authority. |
| `nucleus.output` | Job inspection, exact output observations, structured results, completeness, and retained history. |
| `nucleus.authentication` | Credential ownership, account reads, refresh, attended login, and API-key isolation. |
| `nucleus.quota` | Weekly allowance observations, admission policy, pending work, exhaustion, and recovery. |
| `nucleus.service` | Service readiness, maintenance holds, drain, installation guarantees, retained state, and recovery. |

## Operations

Use `nucleus.execution.operate` to inspect or operate jobs, authentication, or
the service, including installation, backup, and recovery. This broad operating
contract keeps its existing identity and contract version. Its required feature
contracts supply the full behavioral explanations.

Use `nucleus.requester.integrate` to connect an application. Begin with the
application's durable success condition, then select invocation policy,
register tools, submit work, service calls, and interpret the result.

Use `nucleus.develop.change` to change Nucleus. It selects the relevant change
procedure and compatibility, recovery, and validation obligations.

## How the features work together

A requester supplies an explicit invocation and stable request identity.
Nucleus checks readiness and admission, retains the job, and waits for execution
capacity. An active attempt can send tool calls to the requester. The requester
commits its own result before returning a tool response. Nucleus retains runtime
state and output; the requester separately determines domain success.

Quota and maintenance can close admission without making all reads unavailable.
Cancellation and a runtime failure do not undo a requester mutation. A restart
cannot resume the supervised process; requester recovery determines what follows.

Related feature references provide navigation. Only declared dependencies
participate in contract compatibility and complete resolution. Documentation
dependencies do not establish runtime calls or transfer product authority.

The installed `nucleus manual` remains the ecosystem operator manual. Use it
for shared topology, coordinated maintenance, and cross-product recovery order.
Use the feature contracts here for Nucleus behavior. A feature's documented
support and installed release do not establish the live service's readiness.

# Integrate a Nucleus requester

Use this operation to connect an application to shared execution. The
application owns its durable result; Nucleus owns execution. There is no project
registration step. Read `nucleus manual` for shared coordination and
`chancery resolve nucleus.requester.integrate` for this procedure and its
required feature contracts.

## Define the domain boundary

Identify the durable condition that means the application succeeded, the
database, filesystem, or service authoritative for it, and the tools allowed
to mutate that authority. Define duplicate handling, who decides whether a new
attempt is safe, and the evidence that distinguishes domain success from
runtime completion. Keep these decisions in the requester.

## Select the supported interfaces

Use the workspace `nucleus-core` and `nucleus-client` interfaces for Rust
requesters in Cell. Another language can implement the documented HTTP protocol
over the per-user Unix socket. Do not shell out to the human CLI when a typed
client or HTTP surface is available.

Read the feature contracts for the integration's public formats and semantics:

| Contract | Integration decision |
| --- | --- |
| `nucleus.jobs` | Choose requester and job identities, persist correlation, and handle admission and terminal states. |
| `nucleus.invocation` | Set the complete invocation policy and require the corresponding harness capabilities. |
| `nucleus.requester-tools` | Register immutable schemas and toolsets and service calls durably. |
| `nucleus.output` | Interpret runtime evidence without making it a replacement domain record. |
| `nucleus.authentication` | Use managed account access without reading or copying the canonical credential. |
| `nucleus.quota` | Preserve work and deadlines across admission deferral and exhaustion. |
| `nucleus.service` | Check readiness and plan maintenance, restart, and recovery. |

## Connect the execution loop

1. Verify strict Nucleus readiness, required protocol and harness capabilities,
   and application admission prerequisites. A free execution slot is not
   required for admission.
2. Register immutable schemas and toolsets idempotently. Publish a new identity
   when their meaning changes, and keep historical decoders.
3. Select a stable lowercase requester program, a domain-run requester ID,
   and a unique job ID. Persist both correlation directions and the exact typed
   request before submission.
4. Set harness, model, reasoning effort, absolute working directory, workspace
   access, local execution, web search, timeout, launch context, and any dynamic
   toolset explicitly. Keep source content separate from trusted instructions.
5. Assign disjoint directories or worktrees, or serialize concurrent writes.
   Nucleus does not detect overlapping workspaces or external mutation targets.
6. Submit the request. On an ambiguous submission, resend only the same job ID
   and byte-equivalent typed request. A new attempt needs a new ID and the
   requester's decision that it is safe.
7. Tolerate an accepted job with a pending attempt. Its execution timeout starts
   after slot acquisition. A requester-tool wait continues to occupy its slot.
8. Long-poll the durable mailbox while the job is nonterminal. Validate each
   call, commit the requester-owned mutation idempotently, retain the exact
   response, and then post it.
9. Read terminal job state and structured output. Decide success from the
   requester-owned record. Preserve a committed result after runtime failure.

Require `workspace-unrestricted` before submitting invocation policy version two
with `workspaceAccess=unrestricted`. This mode grants current-user filesystem,
process, local socket, and network access without a Codex sandbox restriction.
Approvals remain disabled. Enable local execution separately when commands are
needed. Deploy accepting daemon support before new callers and preserve readers
for retained requests.

## Handle interruption and deferral

Rediscover a pending durable call after requester restart. A Nucleus restart
marks unfinished attempts lost and cannot resume their processes. Inspect domain
state before deciding whether to start another attempt. Do not add a hidden
direct-Codex fallback or invent a missing tool result.

Treat `quota_deferred` as an expected pause. Preserve pending work and its exact
request identity. Use the cached Nucleus quota condition; do not duplicate its
allowance checks or read its credential. Accepted jobs can remain pending during
a pause. Reads, cancellation, mailbox responses, and authentication remain
available as documented by `nucleus.quota`.

Return success with an explicit quota outcome for a deferred scheduled
activation. Keep the requester's deadlines and selection rules. Do not report
an abend or replay expired work automatically. After `quota_exhausted`, inspect
domain effects before authorizing a retry. Quota recovery clears no deployment
hold, operator pause, or Clockwork failure halt.

Upgrade requester clients before enabling the quota gate. Tolerate a daemon
without optional quota health fields. Use coordinated maintenance for cutover.
Nucleus owns credential refresh; requesters never read, refresh, or copy the
canonical credential. Attended login waits for active job and account sessions.

## Verify the integration

Test strict health, capacity reporting, eight active attempts, and pending work
waiting for a slot. Test admission, domain completion, identical and conflicting
job submissions, and identical and conflicting tool results.

Test requester restart with a pending call, daemon loss, queued and active
cancellation, and domain success followed by runtime failure. Verify timeout
after slot acquisition and slot retention while waiting for a requester.

Verify concurrent account reads, serialized refresh, login exclusion,
unsupported invocation combinations, and the absence of a hidden execution
path. Verify requester ownership of work packets, write conflicts, and retries.

Add requester observability, private-state handling, backup coverage, release
ordering, rollback boundaries, and operator documentation. Service readiness
checks must not submit model jobs or create synthetic domain records.

## Authority and privacy

This operation does not authorize unrelated domain changes, production cutover,
release publication, or a retry of failed application work. Use the shared
guarded playbooks for protocol, store, authentication, service, or compatibility
changes. Keep prompts, source content, tool traffic, and retained output within
their private product boundaries.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery records
command identity, time, and thread ID, not arguments, output, or outcomes.
Internal product calls are excluded. Recording errors preserve command results.

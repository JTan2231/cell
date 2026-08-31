# Integrate a Nucleus requester

A Nucleus requester is an application integration, not a registered project.
The application owns the durable result that motivated model work; Nucleus
owns only the shared execution substrate. Begin every integration or shared
boundary change by reading the installed, version-matched manual:

```sh
/Users/joey/.local/bin/nucleus manual
```

## Define the domain boundary first

Before designing an invocation, identify:

- the exact durable condition that means the application operation succeeded;
- the database, filesystem, or service authoritative for that condition;
- the tools allowed to mutate that authority;
- idempotency behavior for duplicate delivery;
- who decides whether another attempt is safe; and
- the proof a person can inspect to distinguish domain success from runtime
  success.

If Nucleus would need to understand application-specific rows or workflow
states to answer those questions, the boundary is wrong.

## Integration contract

Rust requesters in Cell use the workspace `nucleus-core` and `nucleus-client`
sources. Another language may implement the documented HTTP protocol over the
per-user Unix socket. Do not shell out to the human CLI when a typed client or
HTTP surface is available.

Choose a stable lowercase requester program, a domain-run requester ID, and a
unique Nucleus job ID. Persist correlation in both directions. An ambiguous
submission may repeat only the byte-equivalent request under the same job ID;
different content under the same ID is a conflict.

Every invocation policy is explicit: harness, model, reasoning effort,
absolute working directory, workspace access, local execution, web search,
timeout, launch context, and optional dynamic toolset. Require strict health
and the exact protocol and adapter capabilities needed by the requester.

Decoder schemas and toolset registrations are immutable by identity and
digest. When their meaning changes incompatibly, publish a new version and
retain the decoder for historical jobs. Never rewrite an old registration.

## Runtime lifecycle

The normal lifecycle is:

1. Verify strict Nucleus readiness and any domain admission prerequisites.
2. Register immutable schemas and toolsets idempotently.
3. Persist correlation and the exact typed request before ambiguous transport
   can occur.
4. Submit the request.
5. Long-poll the durable requester-tool mailbox while the job is nonterminal.
6. Validate each call, commit the requester-owned mutation idempotently, bind
   the exact result durably, and post it.
7. Read terminal job and structured output state.
8. Decide success from requester-owned state.
9. Use Nucleus output atoms for protocol diagnosis or live reporting, never as
   a replacement domain record.

There is no hidden direct-Codex fallback. A requester restart may rediscover a
pending durable call. A Nucleus restart cannot resume the app-server process;
it marks the attempt lost. Only the requester can authorize a new attempt.

## Required proof

Test strict health, successful admission and domain completion, identical and
conflicting job submissions, identical and conflicting tool results,
requester restart with a pending call, daemon loss, cancellation, timeout,
authentication contention, unsupported invocation combinations, domain
success followed by runtime failure, and absence of a second execution path.

Add requester observability, private-state handling, backup coverage, release
ordering, rollback boundaries, operator documentation, and a real requester
canary. The canary must verify the application's authority, not just a terminal
Nucleus job.

## Authority and authorization

This operation does not authorize domain changes beyond the requested
integration, a production cutover, release publication, or a retry of failed
application work. Shared protocol, store, authentication, service, and
compatibility changes follow the guarded Nucleus playbooks and may require
coordinated requester work.

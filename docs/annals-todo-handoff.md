# Annals and Todo integration handoff

Nucleus is ready to become the shared execution boundary. Annals and Todo are
intentionally not modified in this repository. Their next changes should be
small adapters around their existing domain behavior.

Before either adapter submits work, make the shared daemon's Codex credentials
explicit. The default source is `~/.codex/auth.json`; an existing private
Annals Codex home can be retained by passing its directory through the service
installer's `--codex-home` option.

## Todo

Todo continues to own source provenance, liaison prompt construction,
`create_todo`, the exactly-once domain rule, and its JSON CLI result. Replace the
embedded `codex app-server` process with:

1. Check Nucleus health and register Todo's schema/toolset versions.
2. Submit a read-only job with `builtinTools.localExecution=true`,
   `builtinTools.webSearch=true`, Todo's caller working directory as `cwd`, and
   a Todo request token as `requester.id`.
3. Put the current `tool_server::instructions()` plus Todo's existing developer
   rule in `instructions`; keep the source/direction work item in `prompt`.
4. Long-poll the tool-call mailbox while the job is nonterminal.
5. Run the existing `create_todo` backend for that call and post its raw result.
6. Read the terminal job state and logs for diagnostics.

If Todo durably creates the row and the harness later fails, Todo's durable
domain result remains authoritative, exactly as today. Nucleus should not add a
Todo success column or a second Todo record. Todo's DB also does not need agent
transcript tables.

The CLI must fail clearly when Nucleus is unavailable. A hidden fallback to its
old direct runner would recreate two execution paths and two observability
stories.

## Annals

Annals continues to own filesystem receipt delivery, frozen work and base
selection, the nine liaison tools, `model_runs`/`tool_calls` as domain audit,
reconciliation, recovery policy, and the rule that durable reconciliation wins
over a later runtime error.

Replace only process/protocol supervision:

1. Check Nucleus before claiming an inbox receipt.
2. Register the exact Annals liaison toolset and schemas.
3. Submit one job with both built-in tool flags false and the Annals model-run
   token as `requester.id`. Put Annals's existing base and developer liaison
   rules in `instructions`; retain the frozen work/corpus input in `prompt`.
4. Service pending calls by dispatching to the existing strict Annals tool
   backend and post each result.
5. Preserve Annals's existing attempt/recovery decision above Nucleus. Each
   Nucleus job itself still has one attempt and no retry.
6. Rebuild the usage/report surface by querying Nucleus jobs by that model-run
   token and decoding the referenced Codex schema.

Annals should keep its `model_runs` and `tool_calls`: those establish domain
intent and reconciliation, while Nucleus establishes runtime and protocol
history. The old `annals-usage` raw run/event store becomes redundant once its
reporting query reads Nucleus. Budget policy may remain in Annals if it affects
whether domain work is admitted.

Annals should not move its inbox queue into Nucleus, and Nucleus should not learn
about work IDs, corpus revisions, reconciliation, recovery attempts, or Annals
tool semantics.

## Shared acceptance checks

- Killing and restarting `nucleusd` marks an in-flight attempt `lost`; neither
  caller needs `ps` inspection to explain the state.
- A job blocked on a requester tool is visible as `waiting_on_requester`, with a
  durable pending call that survives requester restart.
- Every Codex stdout JSONL value resolves to the captured schema for the exact
  harness version.
- Querying by `(requester.program, requester.id)` returns all runtime records
  for one Todo or Annals domain run.
- Cancellation terminates the Codex process group and records the request and
  terminal transition.
- Duplicate job submissions and duplicate tool results are idempotent only when
  their digests/content match; conflicting reuse is rejected.

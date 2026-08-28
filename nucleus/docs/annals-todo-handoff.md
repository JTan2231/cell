# Annals and Todo integration handoff

Nucleus exposes the shared execution boundary used by the Annals and Todo
adapters. Their domain behavior remains in their own repositories; the public
contract below is the deployment and compatibility boundary.

Before either adapter submits work, import the currently signed-in Annals Codex
home with `nucleus service install --codex-home <annals-codex-home>`. The source
is copied into Nucleus state; it is not retained as the daemon's runtime home.
Afterward, login, refresh, account preflight, and account usage all run under
Nucleus's one exclusive credential lease.

## Todo

Todo continues to own source provenance, liaison prompt construction,
`create_todo`, the exactly-once domain rule, and its JSON CLI result. Its Nucleus
adapter performs this flow:

1. Require strict Nucleus health and register Todo's schema/toolset versions.
2. Register a complete caller environment snapshot with the memory-only launch
   context endpoint.
3. Submit a read-only job referencing that launch context, with
   `builtinTools.localExecution=true`,
   `builtinTools.webSearch=true`, Todo's caller working directory as `cwd`, and
   a Todo request token as `requester.id`.
4. Put the current base instructions in `instructions`, the existing developer
   rule in `developerInstructions`, and the source/direction work item in
   `prompt`.
5. Long-poll the tool-call mailbox while the job is nonterminal.
6. Run the existing `create_todo` backend for that call and post its raw result.
7. Read the terminal attempt's structured output and logs for diagnostics.

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

Its Nucleus adapter replaces only process/protocol supervision:

1. Call Nucleus account preflight, allowing up to 30 seconds for the credential
   lease, before the first zero-attempt inbox claim. On failure, leave the work
   queued and report `model_auth_unavailable`.
2. Register the exact Annals liaison toolset and schemas.
3. Submit one job with both built-in tool flags false and the Annals model-run
   token as `requester.id`. Put Annals's existing base rules in `instructions`,
   developer liaison rules in `developerInstructions`, and frozen work/corpus
   input in `prompt`.
4. Service pending calls by dispatching to the existing strict Annals tool
   backend and post each result.
5. Preserve Annals's existing attempt/recovery decision above Nucleus. Each
   Nucleus job itself still has one attempt and no retry.
6. Read the attempt's structured final response and query schema-bound logs by
   that model-run token for the usage/report surface. Budget and doctor account
   reads use `waitSeconds=0` and report `authentication_busy` immediately.
7. Delegate attended login to `nucleus auth login --device-auth`.

Annals keeps its `model_runs` and `tool_calls`: those establish domain intent and
reconciliation, while Nucleus establishes runtime and protocol history. For
compatibility with existing reports, the adapter materializes Nucleus's
schema-bound log records into the retained `annals-usage` run/event database;
that database is a reporting projection rather than a second invocation source.
Budget policy remains in Annals because it affects whether domain work is
admitted.

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
- Duplicate job submissions and duplicate tool results are idempotent only when
  their digests/content match; conflicting reuse is rejected.
- A Todo launch context is consumed only by a fresh admitted attempt, never
  persisted, and replaces rather than overlays the daemon environment.
- A Codex refresh produced by a job is durably copied into Nucleus's
  authoritative `auth.json` before another credential user can start.
- `nucleus health` exits nonzero unless the daemon is compatible,
  authenticated, and accepting jobs.

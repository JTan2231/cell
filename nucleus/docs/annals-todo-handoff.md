# Annals and Todo integration handoff

Nucleus exposes the shared execution boundary used by the Annals and Todo
adapters. Their domain behavior remains in their own product trees; the public
contract below is the deployment and compatibility boundary.

Before either adapter submits work, import the currently signed-in Annals Codex
home with `nucleus service install --codex-home <annals-codex-home>`. The source
is copied into Nucleus state; it is not retained as the daemon's runtime home.
Afterward, Nucleus remains the only credential authority. Jobs receive
in-memory managed access tokens, concurrent refresh requests are coalesced at
the authoritative home, account reads use the short canonical credential
boundary, and attended login waits for active job sessions to settle.

## Todo

Todo owns `cN` concern provenance, pending and decided `rN` routing, stable
`tN` identities and direction history, dated `aN` assessments, proposed or
accepted `dN` designs, requester/tool-call correlation, and the explicit human
authorization boundary. Nucleus owns only admission, runtime state,
authentication, Codex compatibility, the tool mailbox, and raw stdout atoms.

Todo registers three current immutable requester toolsets:

- `todo/concern-routing/1` proposes one pending `rN` against frozen candidates;
- `todo/situation-assessment/1` records one immutable `aN` against frozen
  evidence and authority bases; and
- `todo/design-reconciliation/1` records or corrects one basis-bound `dN`
  draft.

The historical Todo `create_todo` schema and toolset remain immutable so old
registrations retain their meaning. Current `todo new` first captures a `cN`
deterministically, then uses concern routing; it does not run the historical
model-authorized creation contract.

For each current stage, Todo's Nucleus adapter performs this flow:

1. Require strict Nucleus health and register the exact stage schemas and
   toolset idempotently.
2. Submit a closed job with `workspaceAccess=none`,
   `builtinTools.localExecution=false`, `builtinTools.webSearch=false`, no
   launch context, and the Todo stage request token as `requester.id`.
3. Put stable stage policy in `instructions`/`developerInstructions` and only
   the frozen stage input in `prompt`.
4. Long-poll the tool-call mailbox while the job is nonterminal.
5. Validate each call against the admitted job, stage, schema, and frozen
   basis; commit its exact Todo domain result before posting the response.
6. Read terminal `JobV1` state and its derived `AttemptOutputV1`.
   `terminalMessage` carries the bounded failure diagnostic; Todo does not need
   the output-ledger endpoint for execution state.

`routing accept`, `routing reject`, `design accept`, and `design reject` bypass
Nucleus. They require explicit source provenance and recheck their recorded
bases in Todo's authorization transaction. A model tool call, final prose, a
ready draft, or a completed Nucleus job is never authorization.

If Todo durably records a proposal, assessment, or draft operation and the
harness later fails, that committed domain result remains authoritative.
Nucleus should not add Todo success or domain-state columns, and Todo does not
need agent transcript tables.

The CLI must fail clearly when Nucleus is unavailable. A hidden fallback to its
old direct runner would recreate two execution paths and two observability
stories.

## Annals

Annals continues to own filesystem receipt delivery, frozen work and base
selection, the nine liaison tools, `model_runs`/`tool_calls` as domain audit,
reconciliation, recovery policy, and the rule that durable reconciliation wins
over a later runtime error.

Its Nucleus adapter replaces only process/protocol supervision:

1. Call Nucleus account preflight, allowing up to 30 seconds for the canonical
   credential operation, before the first zero-attempt inbox claim. On failure,
   leave the work queued and report `model_auth_unavailable`.
2. Register the exact Annals liaison toolset and schemas.
3. Submit one job with both built-in tool flags false and the Annals model-run
   token as `requester.id`. Put Annals's existing base rules in `instructions`,
   developer liaison rules in `developerInstructions`, and frozen work/corpus
   input in `prompt`.
4. Service pending calls by dispatching to the existing strict Annals tool
   backend and post each result.
5. Preserve Annals's existing attempt/recovery decision above Nucleus. Each
   Nucleus job itself still has one attempt and no retry.
6. Read the attempt's derived final response and calculate the live usage/report
   surface from Nucleus's output atoms found by that model-run token. Budget and
   doctor account reads use `waitSeconds=0` and report `authentication_busy`
   immediately.
7. Delegate attended login to `nucleus auth login --device-auth`.

Annals keeps its `model_runs` and `tool_calls`: those establish domain intent and
reconciliation, while Nucleus establishes runtime authority and exact stdout
observations. Annals Usage does not retain a second run/event reporting
database; it joins Annals attribution to Nucleus atoms and calculates usage,
coverage, and totals when read. Budget policy remains in Annals because it
affects whether domain work is admitted.

Annals should not move its inbox queue into Nucleus, and Nucleus should not learn
about work IDs, corpus revisions, reconciliation, recovery attempts, or Annals
tool semantics.

## Shared acceptance checks

- Killing and restarting `nucleusd` marks an in-flight attempt `lost`; neither
  caller needs `ps` inspection to explain the state.
- A job blocked on a requester tool is visible as `waiting_on_requester`, with a
  durable pending call that survives requester restart.
- Every Codex stdout JSONL value has exactly one byte-exact Nucleus atom. The
  log API derives its Codex schema envelope from the owning attempt; bytes that
  cannot be embedded as the identical raw JSON value use the reversible
  Nucleus base64 envelope.
- Querying by `(requester.program, requester.id)` returns all runtime records
  for one Todo or Annals domain run.
- Duplicate job submissions and duplicate tool results are idempotent only when
  their digests/content match; conflicting reuse is rejected.
- A current Todo v2 request has no launch context, no workspace, no builtin
  local execution, and no inherited caller environment; it can inspect only
  the frozen material exposed by its admitted stage tools and prompt.
- Eight independent jobs can own live Codex app-server processes at once. A
  ninth remains accepted and pending, a requester-tool wait keeps its slot, and
  queued cancellation never starts Codex.
- A burst of managed-auth 401 callbacks advances Nucleus's authoritative
  `auth.json` once and returns the new in-memory access-token generation to all
  affected jobs without exposing the refresh token.
- `nucleus health` exits nonzero unless the daemon is compatible,
  authenticated, and accepting jobs.

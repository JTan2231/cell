# Annals and Todo integration handoff

Nucleus provides shared execution for the Annals and Todo adapters. Each
product owns its domain behavior. This contract defines their deployment and
compatibility boundary.

Before either adapter submits work, import the currently signed-in Annals Codex
home with `nucleus service install --codex-home <annals-codex-home>`. Nucleus
copies the source into its own state. The source is not the daemon's runtime
home. After import, Nucleus is the only credential authority. Jobs receive
managed access tokens in memory. Concurrent refresh requests share one refresh
at the authoritative home. Account reads use the short canonical credential
boundary. Attended login waits for active job sessions to settle.

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

For each current stage, Todo's Nucleus adapter:

1. Requires strict Nucleus health and registers the exact stage schemas and
   toolset idempotently.
2. Submits a closed job with `workspaceAccess=none`,
   `builtinTools.localExecution=false`, `builtinTools.webSearch=false`, no
   launch context, and the Todo stage request token as `requester.id`.
3. Puts stable stage policy in `instructions`/`developerInstructions` and only
   the frozen stage input in `prompt`.
4. Long-polls the tool-call mailbox while the job is nonterminal.
5. Validates each call against the admitted job, stage, schema, and frozen
   basis. It commits the exact Todo domain result before posting the response.
6. Reads terminal `JobV1` state and its derived `AttemptOutputV1`.
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

If Nucleus is unavailable, the CLI must report failure. It must not fall back
to the old direct runner and create a second execution and reporting path.

## Annals

Annals continues to own filesystem receipt delivery, frozen work and base
selection, the nine liaison tools, `model_runs`/`tool_calls` as domain audit,
reconciliation, recovery policy, and the rule that durable reconciliation wins
over a later runtime error.

Its Nucleus adapter replaces only process/protocol supervision. The adapter:

1. Calls Nucleus account preflight, allowing up to 30 seconds for the canonical
   credential operation, before the first zero-attempt inbox claim. On failure,
   leaves the work queued and reports `model_auth_unavailable`.
2. Registers the exact Annals liaison toolset and schemas.
3. Submits one job with both built-in tool flags false and the Annals model-run
   token as `requester.id`. It puts Annals's base rules in `instructions`,
   developer liaison rules in `developerInstructions`, and frozen work/corpus
   input in `prompt`.
4. Services pending calls through the strict Annals tool backend and posts each
   result.
5. Preserves Annals's attempt and recovery policy. Each
   Nucleus job itself still has one attempt and no retry.
6. Reads the attempt's derived final response and calculates live usage reports
   from Nucleus output atoms selected by that model-run token. Budget and
   doctor account reads use `waitSeconds=0` and report `authentication_busy`
   immediately.
7. Delegates attended login to `nucleus auth login --device-auth`.

Annals keeps `model_runs` and `tool_calls` to record domain intent and
reconciliation. Nucleus records runtime state and exact stdout observations.
Annals Usage joins Annals attribution to Nucleus atoms and calculates usage,
coverage, and totals when read. It retains no second run/event reporting
database. Annals owns budget policy because that policy controls domain
admission.

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

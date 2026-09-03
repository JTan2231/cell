# Change CRM

Use this operation for changes to CRM code, schema, CLI/JSON behavior,
requester/toolset, tests, documentation, provider contracts, packaging, or
release tooling. Ordinary case use belongs to `crm.case.maintain` or
`crm.library.explore`; installed operation belongs to
`crm.steward.operate`.

This operation does not authorize `release.sh`, deployment, Semantics
registration, database migration, a real requester run, or retrying failed
work.

## Start with authority

Read `crm/AGENTS.md` and the affected architecture, CLI, data-model,
installation, and Chancery contracts. When CRM is registered, query its
Semantics repository before analysis or change; before registration, use the
Cell repository for shared terms.

For any Nucleus requester, invocation, toolset, prompt, permission, correlation,
idempotency, or recovery change, read:

```sh
/Users/joey/.local/bin/nucleus manual
/Users/joey/.local/bin/chancery show nucleus.requester.integrate
/Users/joey/.local/bin/chancery resolve nucleus.requester.integrate
```

Preserve reported readiness, unspecified, unsupported, and dependency outcomes
rather than filling gaps from schemas or implementation code.

## Fixed version-0.1 boundary

CRM is one private local SQLite library. All retained Markdown, intake,
request JSON, and tool-result content is database `TEXT`; input files and
standard input are transient transport. There is no filesystem content tree.

The public CLI consists of initialization/doctor, case creation and reads,
lexical search, tell, and update list/show/wait/resume/retry. `tell` commits one
delivery plus queued update before returning and then launches a hidden worker.
There is no daemon, schedule, crawler, contact sender, automatic retry, or
direct-Codex fallback.

Every immutable case revision has exactly:

- complete Markdown;
- stage in `research|warranted|contacted|connected|helped|closed`;
- nullable advisory; and
- summary.

A non-null advisory is conspicuous on case list/search/show/history, tell
acknowledgment, and update list/show/wait/resume/retry, and never blocks an
otherwise valid operation. Human output uses the fixed non-blocking attention
banner; JSON carries attention plus text. Stages and advisories are CRM state,
not external truth, contact permission, or guaranteed outcomes.

The hidden worker uses Codex model `gpt-5.6-terra`, medium reasoning, a
1,200-second timeout, and immutable toolset `crm/case-steward/1`, with frozen
base and delivery in its prompt and exactly one managed tool,
`submit_case_revision`. Workspace access, local execution, web search, and
launch context are disabled. CRM's guarded atomic revision commit is domain
success; Nucleus completion and model prose are not. Result-post acknowledgment
and terminal Nucleus state/detail remain distinct update evidence, and a later
runtime failure cannot reverse the committed revision.

## Change procedure

1. Locate the smallest owning module and every affected public boundary.
2. Decide whether the change alters database schema, JSON output contract, fixed
   stages, immutable requester/tool meaning, retained privacy, migration,
   packaging, or another product's contract.
3. Implement with isolated synthetic fixtures. Never use a real CRM database,
   person, job-search note, prompt, or tool result in tests or artifacts.
4. Keep database and Nucleus identities bidirectionally correlated. Persist the
   exact request before ambiguous admission and make managed-tool delivery
   replay-safe. Recheck job identity and nonterminality before each domain tool
   dispatch.
5. Preserve the base guard and one transaction for revision, tool receipt,
   case head, update result, and steward-update domain success.
6. Update code, schema, architecture, CLI, data model, installation guide,
   packaging tests, Chancery entry/manual, and shared operator documentation
   wherever behavior actually changes.
7. Run `crm/ci.sh` and inspect its result. Do not infer release or deployment
   authority from a green build.

## Compatibility and migration

Database schema, JSON output contract, provider release, Nucleus protocol,
request schema, and toolset identity are separate compatibility axes. An
incompatible tool argument/result/meaning change publishes a new immutable
toolset or schema identity and keeps the old decoder for retained jobs. Never
rewrite an installed registration.

A database schema change requires worker quiescence, a SQLite-aware backup
including applicable sidecars, explicit migration, an old-state fixture,
post-migration integrity proof, and database-aware rollback. The installer may
never perform that migration implicitly.

Ambiguous admission reuses only a byte-identical typed request with the same
job ID. Resume preserves recoverable work. A genuinely new retry requires
positive terminal evidence, no committed revision, a predecessor link, and a
new job identity. Domain success survives later runtime failure.

## Packaging and contract checks

The crate version and `chancery/provider.json` release must match. The package
contains only the binary, its deployer, canonical manifest, and the exact CRM
provider bundle under `share/chancery/crm`. The installer owns only:

```text
~/.local/bin/crm
~/Library/Application Support/CRM/install/
~/Library/Application Support/Chancery/providers/crm
```

It must refuse foreign/tampered selectors and restore command/provider views
together on a pre-commit failure. It must never open, initialize, migrate,
inspect, back up, or delete `crm.db`.

Provider scope is schema 3 and complete for the four supported entries. Every
entry must retain a complete normalized promise, explicit gaps, compatible
dependencies, and a matching detailed manual. CRM runtime never depends on
Chancery.

## Required proof

As affected, tests must cover exact intake retention, omitted-input creation,
all six stages, immutable revisions, stale-base refusal, deterministic reads,
tell-before-launch durability, strict Nucleus policy, immutable registration,
duplicate admission/tool results, pending-call recovery, daemon loss,
cancellation and timeout, explicit retry, committed-domain/runtime-failure
distinction, visible non-blocking advisory behavior, permissions, and no second
runner.

Stop rather than weakening an invariant, hiding a source/advisory boundary,
inventing readiness, silently migrating state, or broadening CRM into a crawler,
sender, scheduler, generic people graph, or general autonomous agent.

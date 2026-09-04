# Architecture

## Authority

Vizier owns implementation-run admission, the exact input documents, the
planning and packet graph, Nucleus correlations, isolated workspaces, Git
candidate identities, review dispositions, bounded remediation, configured
gate evidence, recovery, and domain success.

Nucleus owns bounded Codex execution, authentication, job and attempt state,
cancellation, and managed-tool transport. A Nucleus terminal job or model
message is evidence only. It cannot advance a Vizier run without the expected
validated Vizier record.

The caller owns the implementation brief, ordered contract-unit boundaries,
terminology snapshot, repository, source revision, gate commands, and authority
for the requested change. Git owns source-object identity and repository
operations, but no installed Chancery contract describes that reliance.

## Opaque documents

The brief, terminology snapshot, contract units, plans, packet instructions,
handoffs, reviews, rationale, and remediation instructions are exact opaque
UTF-8 Markdown. Vizier records bytes, identity, order, lineage, and SHA-256 but
does not parse headings, requirements, findings, or acceptance criteria.

The terminology document is frozen by the caller from the applicable
Semantics repository before submission. Vizier and its Nucleus jobs do not
call Semantics. A changed terminology snapshot or contract set requires a
successor run.

## Finite workflow

1. One read-only planner examines each contract unit against the complete
   frozen input set and exact Git basis.
2. One assembler produces a many-to-many packet graph. Contract units are not
   assumed to be implementation packets.
3. One independent plan reviewer checks the assembled plan set.
4. Ready packets are implemented in disjoint Vizier-owned Git worktrees.
5. A different invocation independently reviews each frozen packet candidate.
6. Accepted packet candidates are integrated into one exact candidate.
7. Caller-supplied gates run against that candidate.
8. One independent integrated reviewer checks the complete frozen contract set
   and exact integrated candidate.

An implementor or integrator never accepts its own edits. The broad review
budget is one plan-set review, one review per packet candidate, and one final
integrated review.

## Findings and remediation

Review routing has three mechanical dispositions:

- `accepted` permits that exact candidate to advance;
- `changes_requested` permits a successor candidate only for a finding anchored
  in an existing contract or accepted packet criterion;
- `blocked` returns authority ambiguity, an unstated requirement, missing
  evidence, or wider scope to the caller.

The review body remains Markdown. Advisories do not create work. Remediation is
limited to the cited finding, changed code, and affected seams, followed by a
targeted recheck rather than a new broad audit. Automatic remediation is
bounded by the submitted positive round limit and defaults to one round;
exhaustion leaves the run `needs_attention`.

## Candidate and success

Every writer receives an isolated worktree based on an exact Git identity.
Vizier freezes the resulting source identity after execution; a mutable
worktree or handoff message is not a candidate. Accepted packet candidates are
combined in an integration worktree without moving the caller's branch.

A run succeeds only when the accepted delegation revision exists, every
required packet has an independently accepted candidate, the exact integrated
candidate passes every configured gate, and an independent final review
accepts that candidate. Vizier never pushes, releases, deploys, or silently
applies the result to the caller's branch.

Vizier is a resumable one-shot coordinator, not a daemon, scheduler, general
workflow engine, requirements parser, deployment orchestrator, or release
gate.

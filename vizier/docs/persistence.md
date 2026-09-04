# Persistence and recovery

## Private ledger

Vizier stores its workflow authority in one caller-selected private SQLite
ledger. The ledger records mechanical facts: runs, exact Markdown documents and
digests, contract ordering, packet and dependency links, invocation roles,
Nucleus jobs and attempts, workspace leases, Git bases and candidates, review
dispositions, remediation lineage, gate results, and terminal state.

It does not turn Markdown headings, requirements, plan steps, review findings,
severity, or rationale into structured domain fields. Documents are immutable
after admission; changed bytes create a successor document or run rather than
rewriting history.

The ledger and correlated Nucleus records may retain private briefs, contracts,
terminology, plans, source context, diffs, prompts, tool calls, and reviews.
Release and Chancery bundles contain none of those bodies. Direct SQLite
editing is unsupported.

## Identity and consistency

A run binds the ordered document digests, resolved Git source identity,
repository, policy, and request key. A Nucleus job ID binds one exact persisted
request. A candidate binds an exact base, resulting Git identity, producing
attempt, packet or integration subject, and handoff document. A review always
names one exact review subject: plan review binds the exact assembled
delegation and plan revision, while packet and integrated review bind an exact
Git candidate.

Vizier validates a managed-tool call against the expected run, role, subject,
candidate, and state before committing it. Identical redelivery returns the
already bound result; conflicting redelivery changes nothing. Nucleus parent
metadata is provenance only and never represents Vizier dependencies.

## Restart recovery

After a Vizier process interruption:

1. inspect `run show RUN_ID` and any selected `attempt show ATTEMPT_ID`;
2. use `run wait RUN_ID` to observe or service the stored in-flight work;
3. use `run resume RUN_ID` when the durable run is recoverable;
4. use `attempt retry ATTEMPT_ID` only after the prior attempt is positively
   terminal and Vizier accepts a successor as safe.

Vizier rediscovers durable pending Nucleus tool calls and posts only the
previously committed result. A Nucleus restart can leave unfinished attempts
`lost`; Codex execution cannot resume. A replacement receives a new job
identity unless it is the byte-identical retry of ambiguous admission.

An interrupted, cancelled, timed-out, or lost writer may leave worktree
changes. Vizier inspects or quarantines that workspace before another writer is
admitted. It never assumes that cancellation rolled source changes back.

## Finite failure behavior

`changes_requested` may create only a bounded successor of the same
review-subject type and a targeted recheck bound to that successor.
An exhausted remediation allowance, unanchored finding, missing authority,
merge conflict outside an accepted packet, unsafe workspace, failed gate, or
unrecoverable execution leaves explicit `needs_attention` rather than silently
expanding scope or retrying forever.

No current command deletes run history, correlated Nucleus history, retained
Git candidates, or private state. Backup, pruning, and destructive recovery are
outside the version-one public interface.

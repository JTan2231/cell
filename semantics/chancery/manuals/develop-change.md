# Change Semantics safely

Use this procedure for a scoped Semantics source, schema, effect, worker,
requester, prompt, CLI, packaging, lifecycle, or documentation change. Semantics
owns its domain and published contracts. Development does not itself authorize
release publication, deployment outside a submitted CI manager job, upstream
mutation, or retained-state deletion.

## Establish the change boundary

1. Read `semantics/AGENTS.md`, the registered project's semantic repository,
   and `chancery resolve semantics.develop.change`.
2. Read the owning feature for each affected promise: `semantics.repository.explore`,
   `semantics.projects`, `semantics.reconciliation`, or `semantics.service`.
3. Read `nucleus manual` before public contract, persistent-state, immutable-tool,
   requester, lifecycle, or packaging changes. Read affected Annals, Conversations,
   Nucleus requester, and Clockwork contracts.
4. Identify changes to identity, cursors, replay, intake, receipt consistency,
   permissions, privacy, ownership, or recovery before editing.
5. Stop if a required fact cannot be obtained through a supported contract or
   recovery lacks a deterministic boundary.

Project source and tests own actual behavior. Feature contracts own the full
public explanation. Operations own procedures and consequential checkpoints.
Update the provider overview, required dependencies, entry points, and exact
release packaging when affected. Do not create another explanation in old docs.

## Preserve the established boundaries

- Keep stable project/concept identity, separate opaque Annals and Decisions
  cursors, contiguous immutable revisions, and typed-effect replay.
- Preserve historical source bytes, grounds, assignments, schemas, and decoders.
  New document grounding, when supplied, names its exact library/event/document.
- Keep model work in a neutral cwd with workspace `none`, no shell/web, one
  immutable managed tool, and no authority outside Semantics validation.
- Persist exact requester/job identity across ambiguous transport. Commit tool
  receipt and domain result before acknowledgement. Reject conflicting replay.
- Preserve paused-project rejection of late commits, serial worker execution,
  and safe retry refusal for active or uncertain jobs.
- Keep catalog discovery out of runtime execution. Usage recording remains a
  separate best-effort library and cannot change command results.

For changed tool input, result, instructions, or semantic meaning, publish new
immutable schema IDs or a new toolset version. Never reinterpret persisted
correlations in place. Prompt text edits retain frozen selection semantics.

For a SQLite change, add an explicit versioned migration. Before candidate
access, disable owned scheduling, suspend public work, hold the worker lock,
prove SQLite closed, and privately back up database plus `-wal`, `-shm`, and
`-journal`. Prove rollback after candidate mutation and later activation failure.
Never open incompatible state with an older binary or discard committed work.

## Validate and submit

Use synthetic values for the retained in-memory tests. File, subprocess, socket,
and worker-lock tests are removed. Test data and logs contain no real user source,
credentials, prompts, private paths, or tool payloads.

The internal Semantics gate is offline and has its documented 60-second deadline.
It uses shared shell and plist syntax checks, provider validation and version
matching, rustfmt, clippy, selected Rust tests, rustdoc, and release build.
The shared catalog gate checks contract dependency compatibility. Select
in-memory checks for the changed boundary. Focused development checks do not
replace the manager outcome.

Commit the intended change and submit it from the Cell root:

```sh
./ci.sh submit COMMIT
```

The installed manager integrates, validates, attempts bounded repairs, deploys,
and emails the outcome. Verify its retained job outcome before completion.
Keep project operations, feature contracts, provider metadata, packaging, and
shared coordination documentation synchronized where the change affects them.
A requested draft may remain uncommitted until delivery is authorized.

Stop before a candidate could expose project workspace, body-bearing logs,
model-authoritative mutations, foreign selectors, or unsupported state access.
Keep failed transaction evidence and maintenance for product recovery rather
than forcing progress. Publication and manual deployment remain separate actions.

## Command usage

CLI usage recording requires nonempty `CODEX_THREAD_ID`. It records command
identity, time, and thread ID, not arguments, output, or outcomes. Internal calls
are excluded. Recording errors preserve command results.

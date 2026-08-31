# Architecture

Weaver is a durable requester around five independent Nucleus invocations. It
is not part of Cell and Nucleus is not its workflow engine.

## Authority boundaries

| Component | Authority |
| --- | --- |
| Narrative repository | Basis, active output brief, canonical sources, prompts, current stage outputs, and mechanical narrative validity |
| Weaver | Current workflow admission, stage order, repository input snapshots and output writes, exact Nucleus job correlations, cancellation intent, and recovery |
| Weaver Chancery bundle | Versioned capability claims and detailed operating manuals for global discovery; no execution or repository authority |
| Nucleus | Content-only invocation admission, Codex supervision, authentication, cancellation, job state, structured final output, and raw prompt/protocol history |
| Interactive caller | Starts a detached Weaver worker with the file-access context needed for every selected repository read and write |

Nucleus completion alone is not a successful narrative build. Weaver must
persist every required stage output and pass the repository's mechanical
validation. Conversely, Weaver never treats its generated text as a factual
source and never publishes it.

## Capability publication

The product-owned `chancery/` bundle catalogs the outcomes Weaver can actually
serve: building the five-stage narrative, operating its durable workflow, and
changing the implementation under the repository's development contracts. Its
titles and summaries support semantic selection; its manuals name effects,
authorities, recovery paths, dependencies, and explicit non-capabilities. In
particular, they do not turn narrative generation into job search, application
submission, publication, upload, or public-profile editing.

The macOS release contains an immutable copy of this bundle and publishes it
through Weaver's provider selector. Chancery can validate and discover those
documents, but Weaver's CLI and worker never call Chancery. Runtime execution
continues to use only Weaver's own contracts and the separately installed
Nucleus service.

## One workflow

`submit` validates the selected project, creates a stable run identity, and
atomically records one current request before waking the worker. Only one
current workflow may be active. The worker requires strict Nucleus readiness
before it clears any prior generated output.

The worker holds `.run.lock` for the complete pipeline and executes these stages
sequentially:

1. stories;
2. themes;
3. composition;
4. editorial review; and
5. finalization.

Every stage is a fresh Nucleus job using Codex, `gpt-5.6-sol`, max reasoning,
Weaver's absolute private state root as its read-only working directory, local
execution disabled, web disabled, no launch context, no dynamic toolset, and a
one-hour timeout. The Nucleus-launched process receives no repository working
directory and no tool with which to inspect one.

Before constructing a stage request, Weaver reads the exact selected files in
its own process and embeds their labeled contents in the prompt. Every request
contains `AGENTS.md`, `narratives/README.md`, `common.md`, `voice.md`, and its
stage-specific contract. Stages 1--2 contain the basis and named originals but
not the brief; stage 2 additionally contains the current work stories. Stages
3--4 contain the basis, brief, named originals, and their required earlier
outputs. Stage 5 contains only the work stories, draft, and review in addition
to the governing files. These bytes are the immutable input snapshot for that
Nucleus job; the model does not discover or re-read repository paths.

A run ID groups all five jobs; the deterministic stage job ID is the Nucleus
idempotency key. Weaver persists the exact active typed job request before
admission and reuses those bytes on recovery. Workflow order remains entirely
in Weaver rather than being inferred from Nucleus provenance.

The model returns Markdown in its structured final message. Weaver validates a
nonempty response and atomically installs it as that stage's `output.md`; the
model receives no write authority. Before a fresh run, Weaver validates the
complete existing stage tree and removes the old outputs. If a later stage
fails, completed outputs from the new run remain and uncompleted later outputs
remain absent.

## Current operational state

The default private state root is:

```text
~/Library/Application Support/Weaver/
  current.json
  .control.lock
  .run.lock
  .maintenance
  install/
```

`current.json` is atomically replaced and contains the one current workflow,
its stage progress, the exact active typed Nucleus request, and the correlations
required to recover that job. While a stage is active, that request includes
the complete private input snapshot embedded in its prompt. It is cleared from
the current record when the stage settles. A terminal record remains current
until the next submission replaces it.
Weaver creates no completed-run archive, workflow ledger, project registry, or
copy of generated stage output. Nucleus separately retains its own sensitive
job, complete prompt, and protocol history.

`.control.lock` orders short state transitions. `.run.lock` prevents overlapping
workers and is held for the complete pipeline. `.maintenance` is a deployer
gate; ordinary commands must not create or remove it directly.

## Recovery and cancellation

A worker restart reads `current.json`, recovers the persisted active stage
request, and looks up its deterministic Nucleus job ID. It resubmits only that
same request after an ambiguous admission failure. A different attempt requires
a new workflow run and new job IDs.

Weaver fails clearly when Nucleus is unavailable or incompatible. It has no
direct-Codex execution fallback.

A Nucleus daemon restart is different: an unfinished attempt becomes `lost`
and cannot be resumed. Weaver reports that workflow as failed and does not
invent an automatic second model attempt. A timeout, cancellation, conflicting
job identity, malformed output, or failed mechanical check is likewise
terminal for the current run.

`cancel [RUN_ID]` durably records cancellation intent before requesting
cancellation of the current Nucleus job. Supplying the observed run ID prevents
a late command from selecting its successor. Cancellation never means delete
the persisted stage files or Nucleus history.

## Background activation

After the current request is durable, `submit` starts the same Weaver
executable as a detached child with `--state-dir ABSOLUTE_PATH worker run`, the
private state root as its working directory, null standard streams, and a new
process group. The process is a one-shot worker, not a resident daemon.

This direct parent-child activation is deliberate on macOS. The prototype
LaunchAgent could not remove generated outputs under `~/Documents` because
launchd did not carry the required repository file-access authorization. A
second live canary showed that a Nucleus-launched Codex app-server whose
invocation cwd was the protected repository remained stuck in `getcwd` before
the app-server handshake. The repository therefore cannot be either Weaver's
service-owned workspace or Codex's invocation cwd.

A detached child of the submitting or waiting CLI preserves the responsible
interactive process for all repository I/O. It passes content-only requests to
Nucleus, whose Codex invocation stays rooted in Weaver's private state and has
no tools. Weaver installs and relies on no LaunchAgent.

`cancel` activates a worker after recording nonterminal cancellation. A
nonterminal `wait` activates immediately and again every 30 seconds, allowing
recovery after a worker crash, logout, or machine restart. `status` remains
strictly read-only and never starts work.

## Maintenance

`maintenance begin` creates `.maintenance` while holding `.control.lock`, then
waits for `.run.lock` to become available. A concurrent submit either commits
before that ordered boundary or observes maintenance and fails before changing
the current request. An active workflow may finish, but the worker claims no
new work while maintenance exists.

The macOS deployer uses this interface and switches the complete installed
release and Chancery provider documentation only after the active workflow
settles. It validates the candidate while maintenance remains in effect, commits
the cutover, and removes maintenance through the installed CLI. As a one-time
migration, it also boots out and removes the exact `org.weaver.worker` prototype
service and plist. A pre-commit failure restores that service together with the
old release and provider resolution. Nucleus stays running throughout.

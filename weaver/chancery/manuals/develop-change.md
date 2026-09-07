# Change Weaver

Weaver is a five-stage durable narrative requester. It does not search for jobs,
submit applications, operate a browser, publish, or edit public profiles.
Preserve that scope unless a separately authorized product decision changes it.
This contract does not authorize that decision.

## Start from the owning contract

Read `AGENTS.md`, then select the exact current document:

- `docs/architecture.md` for authority, stage execution, private state,
  recovery, activation, and maintenance;
- `docs/cli.md` for command selection and outcome semantics;
- `docs/system-installation.md` for installed layout, deployment transaction,
  activation, and recovery; and
- the selected narrative repository for authored editorial vocabulary, inputs,
  prompts, and mechanical output rules.

Before changing Weaver's Nucleus requester contract, persistent operational
state, service lifecycle, deployment, or compatibility boundary, run:

```sh
/Users/joey/.local/bin/nucleus manual
```

Nucleus owns execution, authentication, job state, and raw protocol history.
Weaver owns current-run admission, stage order, input snapshots, output writes,
validation, cancellation intent, and recovery. The narrative repository owns
authored inputs and current generated outputs.

## Invariants

The detached child of the interactive Weaver CLI performs every repository read
and write. This preserves the caller's macOS file-access context. The
Nucleus-launched Codex process uses Weaver private state as a read-only working
directory and receives only embedded contents, with local execution, web
search, launch context, and dynamic tools disabled.

Only one current run can be active. Weaver persists the exact active typed
request before submission and uses a deterministic stage job ID. If submission
has an ambiguous result, recovery can resubmit only those bytes. A different
model attempt requires a new run. Daemon loss does not trigger an automatic retry.

Weaver must atomically persist each stage output and pass repository validation
before recording a successful build. Earlier successful stage outputs remain
after later failure. Generated text stays in the narrative repository.

## Development and proof

1. Change the smallest owning component.
2. Update operator-facing documentation in the same change whenever the
   requester, state, activation, recovery, compatibility, or deployment behavior
   changes.
3. Extend tests for the exact affected authority and failure boundary.
4. Run:

   ```sh
   cd /Users/joey/rust/cell/weaver
   ./ci.sh
   ```

   Treat that command as Weaver's complete quality gate.

Persistent-state or packaging changes need atomic replacement, tamper
detection, rollback proof, and explicit recovery after a post-commit failure.
A requester change must retain exact correlations, one-attempt semantics,
strict readiness, and the absence of a direct-Codex fallback.

`release.sh` is a publication command. It bumps the package and Chancery
provider release together, runs CI, commits, tags, and pushes. Do not invoke it
without explicit publication intent. Deployment and cancellation of an active
installed run are separate actions too.

Private state, test fixtures, Nucleus jobs, and diagnostics can contain complete
basis, brief, source, prompt, and generated-output content. Release bundles may
contain only Weaver program, deployment, and Chancery documentation bytes;
never package narrative inputs.

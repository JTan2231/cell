# Change Weaver

Weaver is a five-stage durable narrative requester. It is not a live job-search,
application-submission, browser, publication, or public-profile system. Changes
must preserve that product boundary unless a separately authorized product
decision changes it explicitly; this contract does not make that decision.

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

One current run may be active. The exact active typed request is durable before
ambiguous submission and uses a deterministic stage job ID. Worker recovery
may resubmit only those same bytes. A different model attempt requires a new
run; daemon loss is not an automatic retry signal.

Nucleus completion is not Weaver success. Weaver must atomically persist each
stage output and pass repository validation. Earlier successful stage outputs
remain after later failure. Generated text is not factual authority and Weaver
never publishes it.

## Development and proof

1. Change the smallest owning component.
2. Update operator-facing documentation in the same change whenever the
   requester, state, activation, recovery, compatibility, or deployment truth
   changes.
3. Extend tests for the exact affected authority and failure boundary.
4. Run:

   ```sh
   cd /Users/joey/rust/cell/weaver
   ./ci.sh
   ```

   Weaver's full quality gate has a hard 60-second deadline.
5. Run any separately authorized live requester or deployment canary required
   by the changed boundary and inspect Weaver's domain result, not only Nucleus.

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

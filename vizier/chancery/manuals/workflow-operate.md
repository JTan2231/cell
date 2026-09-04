# Operate Vizier

Initialize state explicitly, then diagnose the selected ledger and requester:

```sh
vizier init
vizier --json doctor
```

Inspection is the first recovery step:

```sh
vizier run list
vizier run show RUN_ID
vizier document show DOCUMENT_ID
vizier attempt show ATTEMPT_ID
```

Use `run wait` to observe and service durable in-flight work. Use `run resume` after a coordinator interruption; multi-round recovery resumes from the latest durable revision. Both preserve the frozen run inputs. `needs_attention` is terminal, and wait or resume returns its durable result without creating attempts or restarting a prior review round. `document show` reads exact retained Markdown by default, or supported metadata plus the exact Markdown with `--json`; list/show/status stay body-free. Cancel
only with explicit authority; cancellation records intent and asks Nucleus to
cancel individual jobs but cannot undo committed documents or repository
mutations.

```sh
vizier run wait RUN_ID
vizier run resume RUN_ID
vizier run cancel RUN_ID
```

`attempt retry` creates an explicit successor only after Vizier can establish
that the prior attempt is terminal, is the current resultless leaf, and replacement is safe. Ambiguous
admission instead reuses the byte-identical persisted request and job ID.
There is no automatic terminal retry or direct-Codex fallback.

Deploy only a green authorized candidate:

```sh
vizier/packaging/macos/deploy-user.sh \
  --binary /absolute/path/to/vizier \
  --expected-current absent
```

The deployer changes only Vizier's content-addressed release, command selector,
and provider selector. It does not initialize the database, call Nucleus, run
a workflow or canary, install a service, push, or publish a release. Run
`vizier init` separately after a first installation.

Never edit the ledger, worktrees, Git references, Nucleus state, or installed
selectors manually to force recovery. Retain exact evidence and stop at
`needs_attention` when safe authority cannot be established.

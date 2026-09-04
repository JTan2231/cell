# macOS user installation

Vizier is one independently versioned Cell product. It installs a
content-addressed current-user release, a `~/.local/bin/vizier` command
selector, and one Chancery `providers/vizier` selector. It installs no daemon,
LaunchAgent, schedule, or shared credential.

## Prerequisites

- A compatible installed Nucleus service satisfying
  `nucleus.execution.operate` contract 2 and the capabilities reported by
  `vizier doctor`.
- Git available for the caller-selected repositories and revisions.
- A caller-frozen terminology snapshot obtained through
  `semantics.repository.explore` contract 1 before a run is submitted.
- Chancery only for provider validation and installed discovery; Vizier does
  not call it at runtime.

Build and run Vizier's product gate before deployment. Deployment is
selector-only: it stages the exact binary and unchanged provider bundle, checks
that their release versions match, publishes both owned selectors coherently,
and retains the previous release for rollback. It does not open or initialize
the Vizier ledger, call Nucleus, run a workflow, inspect a repository, or
perform a canary.

```sh
vizier/packaging/macos/deploy-user.sh \
  --binary /absolute/path/to/vizier \
  --expected-current absent
```

For an update, replace `absent` with the previously observed
`releases/64-lowercase-hex-identity`. Omitting `--expected-current` snapshots
the selected generation before waiting for the product lock; explicit expected
state is preferable when deployment is part of a concurrent plan.

Initialize domain state separately after installation:

```sh
/Users/joey/.local/bin/vizier init
/Users/joey/.local/bin/vizier --json doctor
```

Validate discovery separately:

```sh
/Users/joey/.local/bin/chancery show vizier.implementation.delegate
/Users/joey/.local/bin/chancery show vizier.workflow.operate
/Users/joey/.local/bin/chancery show vizier.develop.change
```

Provider validation proves only the installed documentation bundle. Doctor
proves current ledger and requester readiness. A real bounded run is required
to prove delegation domain success.

Rollback changes the binary and provider selectors together and is safe only
while the retained ledger remains compatible with the selected binary. Removing
selectors or releases does not authorize deleting the private ledger,
worktrees, Git candidates, or correlated Nucleus records.

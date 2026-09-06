# Install and configure Cast

Build and verify a candidate through the Cell product gate before installing:

```sh
./ci.sh cast
cast/packaging/macos/deploy-user.sh --binary /absolute/path/to/cast
```

The product-owned deployer stages the exact Rust payload, zsh frontend,
deployer and Chancery provider bundle under
`~/Library/Application Support/Cast/install/releases/HASH`. The installed
`~/.local/bin/cast` and provider selector follow one atomic `current` release.
The deployer takes product and catalog writer locks, refuses foreign selectors,
checks candidate/provider versions and restores prior selectors after a failed
switch. It does not require a Chancery runtime, create the discovery database,
run searches or install a scheduler.

`--expected-current absent|releases/HASH` adds an optimistic selection guard;
`--home PATH` supports an explicitly selected operator home. Deployment keeps
prior releases available. Reinstalling a previously tested candidate restores
its program/documentation release; it does not roll back discovery state.
An abruptly killed deployer can leave its `.update-lock` directory. Confirm
that no Cast deployer is running before removing that stale installation lock
and rerunning the intended tested candidate; never remove another active
writer's lock. Runtime collection uses a separate kernel-backed file lock.

## State and credentials

Initialize the default private state directory explicitly:

```sh
cast init
cast doctor
cast config show
cast config set --file /absolute/path/to/config.json
cast source add https://employer.example/careers --company-id COMPANY_ID
cast source add https://job-boards.greenhouse.io/EMPLOYER
cast source disable SOURCE_ID
```

State defaults to `~/.local/share/cast`; select another directory through
`--state-dir PATH` or `CAST_STATE_DIR`. The database is `cast.sqlite3`; the directory is private mode 0700 and the
database is mode 0600. Config is persisted in database metadata.
The installed wrapper preserves an explicit `CAST_STATE_DIR`, and the CLI flag
can select state independently of environment. Installation state and discovery
state are different recovery units.

The Rust payload reads `THEIRSTACK_API_KEY` and `BRAVE_SEARCH_API_KEY` from its
environment. The installed zsh frontend suppresses trace/output while sourcing
`~/.zshrc`, extracts those two keys and launches with a scrubbed environment:
`HOME`, fixed system `PATH`, optional `CAST_STATE_DIR`, and the two provider
keys. The frontend preserves arguments and standard input. Help/version reads
bypass shell configuration. No key appears in an argument, saved config,
provider contract or command output.

`.zshrc` is user-owned executable shell configuration, so its own commands and
side effects remain the user's responsibility. Local readiness can check
configuration and credential presence; it is not proof of current provider
balance, authentication or successful discovery.

## Collection policy

The default configuration includes TheirStack, Brave and Hacker News queries.
It retains intervals, terms and adapter parameters alongside request caps.
Inspect the current complete config before replacing it. Defaults allow 200
total and 60 daily TheirStack credits, 1,000 monthly and 30 daily Brave requests,
500 HTTP requests per run, 3,000 daily HTTP requests, 600 seconds per run and 50 verifications per run. The total TheirStack allowance belongs
to this Cast state; it is not a provider monthly balance.

A forced run still respects budgets. Configuration changes do not reset
consumed allowance, purchase credits or configure provider billing. New
schedules, billing and downstream workflows remain separate operations.

`source add URL --company-id COMPANY_ID` associates an ordinary website source
with an existing company. For a supported ATS URL, omit `--company-id`: Cast
assigns the canonical provider/tenant identity as its owner and rejects an
explicit company override. A website linking to a board does not establish
that it is the board's employer. Without `--company-id`, an ordinary website URL
creates or reuses a company candidate from its hostname.
`source disable SOURCE_ID` retains the source
and its evidence while removing it from ordinary collection. It does not
delete jobs, dismiss them or imply the employer stopped hiring.

## Recovery

Inspect the run, source-health and budget diagnostics before rerunning failed
work. A missing provider key or unavailable source can leave a run partial while
other successful observations remain durable. Do not erase state to clear a
budget or treat an absent error as evidence of complete collection.

For state collected before the ATS ownership correction, run the explicit local
repair after collection has stopped:

```sh
cast state reconcile-ownership
cast export --json
cast status --json
```

The repair holds the mutation lock and commits one transaction. It assigns ATS
sources and their jobs to the provider/tenant owner, changes unproven older
JSON-LD jobs to `unknown`, marks those sources for identity review, and restores
unverified search candidates' names to their domains when appropriate. It also
quarantines JSON-LD on shared recruiting hosts even when an older parser marked
it owned, and clears misleading shared-host company domains, website URLs and
their identity aliases. JSON
output reports `moved_sources`, `moved_jobs`, `quarantined_jobs` and
`renamed_candidates`, plus `cleared_shared_identities`. Source and job IDs, paid request accounting, run history,
query coverage and cursors remain intact; changed jobs/companies gain revisions.
It makes no network request. Repeating it after repair leaves material records
unchanged, while each invocation advances the snapshot revision. Re-export after
repair and perform a new bounded collection to refresh uncertain observations.

Before state recovery, stop all callers using the selected state directory and
make a private consistent SQLite backup, including live sidecars when relevant.
Restoring an older program alone cannot restore newer domain state. This release
provides no automatic database migration, pruning, destructive reset or state
uninstaller. Use documented configuration and read commands; do not repair
individual database rows or installed content-addressed bundles by hand.

The Cast database, exports and any external diagnostic capture remain private.
Cast has no Nucleus, CRM, Email or computer-use runtime dependency. Chancery
provides installed documentation only and its catalog presence grants neither
execution authority nor proof of provider readiness.

# Install and configure Cast

Build and verify a candidate through the Cell product gate before installing:

```sh
./ci.sh cast
<TESTED_CAST_INSTALL> install --binary <TESTED_CAST_BINARY> \
  --bundle /Users/joey/rust/cell/cast/chancery
```

The product-owned Rust installer stages the exact Rust payload, static zsh
frontend, installer and Chancery provider bundle under
`~/Library/Application Support/Cast/install/releases/HASH`. The installed
`~/.local/bin/cast`, `~/.local/bin/cast-install`, and provider selector follow one
atomic `current` release. The `cell-install-v2` manifest is `manifest.json`;
`package/install` retains the Rust installer.
The deployer takes product and catalog writer locks, refuses foreign selectors,
checks candidate/provider versions and restores prior selectors after a failed
switch. It does not require a Chancery runtime, create the discovery database,
run searches or install a scheduler.

Use `--expected-current absent|releases/HASH` to require the expected current
selection. Use `--home PATH` to select the operator home. Deployment keeps
prior releases available. To recover one, resolve `install/previous` to its
canonical owned release directory, then run a trusted tested
`cast-install recover --release ABSOLUTE_RELEASE_DIRECTORY`. The installer
verifies the retained legacy or `cell-install-v2` release before selection. Do
not execute an unverified retained installer. Program recovery leaves discovery
state unchanged.
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

State defaults to `~/.local/share/cast`. To select another directory, use
`--state-dir PATH` or `CAST_STATE_DIR`. The database is `cast.sqlite3`.
The directory uses mode 0700; the database uses mode 0600. Cast stores
configuration in database metadata.
The installed wrapper preserves an explicit `CAST_STATE_DIR`, and the CLI flag
can select state independently of environment. Installation state and discovery
state are different recovery units.

The Rust payload reads `THEIRSTACK_API_KEY` and `BRAVE_SEARCH_API_KEY` from its
environment. The installed zsh frontend suppresses trace and output while it
sources `~/.zshrc`. It extracts those keys and starts the payload with only:
`HOME`, fixed system `PATH`, optional `CAST_STATE_DIR`, and the two provider
keys. The frontend preserves arguments and standard input. Help/version reads
bypass shell configuration. No key appears in an argument, saved config,
provider contract or command output.

`.zshrc` is user-owned executable shell configuration, so its own commands and
side effects remain the user's responsibility. Local readiness can check
configuration and credential presence. Provider balance, authentication and
collection results require provider interactions.

## Collection policy

The default configuration includes TheirStack, Brave and Hacker News queries.
It retains intervals, terms and adapter parameters alongside request caps.
Inspect the current complete config before replacing it. Defaults allow 200
total and 60 daily TheirStack credits, 1,000 monthly and 30 daily Brave requests,
500 HTTP requests per run, 3,000 daily HTTP requests, 600 seconds per run and
50 careers collections per run. The total TheirStack allowance belongs
to this Cast state; it is not a provider monthly balance.

A forced run still respects budgets. Configuration changes do not reset
consumed allowance, purchase credits or configure provider billing. New
schedules, billing and downstream workflows remain separate operations.

`source add URL --company-id COMPANY_ID` associates an ordinary website source
with an existing company. For a supported ATS URL, omit `--company-id`: Cast
assigns its canonical provider/tenant company identity and rejects an explicit
company override. Without `--company-id`, an ordinary website URL creates or
reuses a company candidate from its hostname.
`source disable SOURCE_ID` removes the source from ordinary collection while
retaining the source, its jobs and its collected data.

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
sources and their jobs to the provider/tenant company, sets older JSON-LD jobs
to `unknown`, marks their sources for the next collection, and restores affected
search-candidate names to their domains. It also applies current adapter rules
to shared recruiting hosts and clears their company domains, website URLs and
identity aliases. JSON output reports `moved_sources`, `moved_jobs`,
`quarantined_jobs`, `renamed_candidates` and `cleared_shared_identities`.
Source and job IDs, paid request accounting, run history, query coverage and
cursors remain intact. Changed jobs and companies gain revisions.
The repair uses stored records. Repeating it leaves material records unchanged,
while each invocation advances the snapshot revision. A later collection uses
the updated associations.

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

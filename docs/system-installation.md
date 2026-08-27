# User-owned macOS installation

Todo is synchronous and owns no LaunchAgent, daemon, root-owned files, log
service, or scheduled task. `todo new` uses the same user's separately
installed Nucleus service, which owns Codex execution and authentication.

## Deploy

Install and authenticate Nucleus first, then build and test Todo and pass its
absolute executable path to the deployer:

```sh
./ci.sh
./packaging/macos/deploy-user.sh \
  --binary "$PWD/target/release/todo"
```

The layout is:

```text
~/.local/bin/todo
~/Library/Application Support/Todo/
  config.toml
  todo.db
  install/
    releases/<content-hash>/
      bin/todo
      libexec/todo
      package/todo
      package/deploy-user.sh
      manifest.txt
    current -> releases/<content-hash>
    previous -> releases/<content-hash>
```

`~/.local/bin/todo` selects `config.toml` when no explicit database or config
selector is present. The config points at `todo.db` and selects high liaison
quality. Nucleus is resolved through `NUCLEUS_SOCKET` when set, or its standard
per-user socket otherwise.

Deployment first verifies the installed Nucleus service is healthy, stages a
complete content-addressed release, writes configuration,
switches `current`, initializes the database on a fresh install, and runs an
installed JSON list smoke test. An update retains the prior release through
`previous`. If initialization or smoke testing fails, the deployer restores
the prior release selector, frontend, previous selector, and configuration.
It never deletes the user's existing database.

Running the same deploy command with a new release binary performs an update.
An identical package reuses its release directory.

## Explicit targets

The installed frontend's default is only a convenience. These continue to
bypass it:

```sh
todo --database /path/to/other.db list
TODO_DATABASE=/path/to/other.db todo list
todo --config /path/to/other.toml list
TODO_CONFIG=/path/to/other.toml todo list
```

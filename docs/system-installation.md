# User-owned macOS installation

Todo is synchronous and needs no LaunchAgent, daemon, root-owned files, log
service, or scheduled task. The installation is owned entirely by the user who
runs Todo and Codex.

## Deploy

Build and test, then pass absolute executable paths to the deployer:

```sh
./ci.sh
./packaging/macos/deploy-user.sh \
  --binary "$PWD/target/release/todo" \
  --codex "$(command -v codex)"
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
selector is present. The config points at `todo.db`, the supplied Codex
executable, and high liaison quality.

Deployment stages a complete content-addressed release, writes configuration,
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

# Nucleus

Nucleus is a per-user, local job coordinator for agent harnesses. A requester
submits a small, versioned invocation contract; Nucleus validates it against the
installed harness, supervises the process, owns its authentication, and retains
the complete schema-bound JSONL record. Requesters keep ownership of their
domain work and execute any domain tools through Nucleus's durable mailbox.

The first adapter is Codex app-server on macOS. Nucleus does not accept shell
commands, arbitrary argv, retries, or workflow graphs. A requester that needs
caller-process environment parity can register a short-lived, memory-only
launch context; those values never enter the job or log database.

## Build and run

```sh
/Users/joey/.cargo/bin/cargo build --release
target/release/nucleusd serve
```

By default the daemon listens on the Unix socket at
`~/Library/Application Support/Nucleus/nucleus.sock` and stores SQLite state in
the same directory. For an isolated foreground instance:

```sh
target/release/nucleusd serve \
  --socket /tmp/nucleus.sock \
  --database /tmp/nucleus.db \
  --codex /opt/homebrew/bin/codex \
  --codex-home /tmp/nucleus-codex-home
```

Install it as the current user's always-on LaunchAgent:

```sh
target/release/nucleus service install \
  --daemon target/release/nucleusd \
  --codex-home "$HOME/path/to/current-signed-in-codex-home"
target/release/nucleus service status
target/release/nucleus health
```

Installation copies the binaries to `~/.local`, writes
`~/Library/LaunchAgents/org.nucleus.daemon.plist`, and loads it with
`launchctl bootstrap`. The daemon remains in the foreground under launchd.
The installer copies `auth.json` from `--codex-home` into
`~/Library/Application Support/Nucleus/codex-home`, writes a minimal private
`config.toml` that selects the file credential store, and configures the
LaunchAgent to use only that Nucleus-owned home. The source home is an import,
not a shared runtime path. Nucleus serializes every job, account read, token
refresh, and attended login with one credential lease; a refresh produced in an
isolated job home is atomically copied back before the lease is released.

If Nucleus has no signed-in credential yet, authenticate the owned home with:

```sh
target/release/nucleus auth login --device-auth
target/release/nucleus account --wait 0
```

`nucleus health` is strict: it prints the readiness document but exits nonzero
unless the daemon is compatible, authenticated, and accepting jobs.

## Operational storage

Nucleus retains job requests and the complete schema-bound app-server protocol
in `~/Library/Application Support/Nucleus/nucleus.db`. Those records can contain
prompts, tool arguments and results, and source content emitted while an agent
researches. Version 1 has no automatic retention limit or pruning policy, so
operators should monitor free space and treat the database as sensitive local
state. Back it up with a SQLite-aware backup while the service is stopped (or
include its WAL consistently); copying only the main database file while the
daemon is running is not a complete backup. The LaunchAgent's stdout and stderr
files under `~/Library/Logs/Nucleus` likewise need the host's normal log
rotation policy.

## Submit a smoke job

```sh
nucleus jobs submit examples/job.smoke.json
nucleus jobs show nucleus-smoke-01
nucleus jobs logs --follow nucleus-smoke-01
```

The Todo adapter uses the same checked-in requester contracts; these commands
exercise the underlying registration and mailbox surface directly:

```sh
nucleus schemas register examples/schema.todo-create-result.json
nucleus toolsets register examples/toolset.todo.json
nucleus jobs submit examples/job.todo.json
nucleus jobs show todo-research-2026-08-26-01
nucleus tool-calls pending --wait 30 todo-research-2026-08-26-01
```

The HTTP API is also available directly over the Unix socket:

```sh
curl --unix-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock" \
  -H 'content-type: application/json' \
  --data-binary @examples/job.smoke.json \
  http://nucleus.local/v1/jobs
```

See [the runtime contract](docs/runtime-contract.md) for exact request and log
examples, and [the integration handoff](docs/annals-todo-handoff.md) for the
deployed Annals and Todo adapter contract.

## Scope

Nucleus owns admission, compatibility checking, execution lifecycle,
cancellation, raw protocol retention, and retrieval. It reports that an agent
turn completed; it cannot decide that an Annals reconciliation or Todo creation
was successful. That remains authoritative in the requester's database.

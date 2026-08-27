# Nucleus

Nucleus is a per-user, local job coordinator for agent harnesses. A requester
submits a small, versioned invocation contract; Nucleus validates it against the
installed harness, supervises the process, and retains the complete
schema-bound JSONL record. Requesters keep ownership of their domain work and
execute any domain tools through Nucleus's durable mailbox.

The first adapter is Codex app-server on macOS. Nucleus does not accept shell
commands, arbitrary argv, environment variables, retries, or workflow graphs.

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
  --codex /opt/homebrew/bin/codex
```

Install it as the current user's always-on LaunchAgent:

```sh
target/release/nucleus service install --daemon target/release/nucleusd
target/release/nucleus service status
```

Installation copies the binaries to `~/.local`, writes
`~/Library/LaunchAgents/org.nucleus.daemon.plist`, and loads it with
`launchctl bootstrap`. The daemon remains in the foreground under launchd.
By default jobs copy authentication from `~/.codex/auth.json` into their
isolated harness home. If an existing installation keeps that file in a private
Codex home, pass `--codex-home /absolute/path/to/codex-home` to the service
install command; the LaunchAgent records only that directory path, not
credentials.

## Submit a smoke job

```sh
nucleus jobs submit examples/job.smoke.json
nucleus jobs show nucleus-smoke-01
nucleus jobs logs --follow nucleus-smoke-01
```

The pending Todo adapter can use the checked-in requester contracts after it is
ready to service the tool mailbox:

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
remaining Annals and Todo changes.

## Scope

Nucleus owns admission, compatibility checking, execution lifecycle,
cancellation, raw protocol retention, and retrieval. It reports that an agent
turn completed; it cannot decide that an Annals reconciliation or Todo creation
was successful. That remains authoritative in the requester's database.

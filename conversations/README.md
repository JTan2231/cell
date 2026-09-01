# Conversations

Conversations is a local CLI and Rust library for exploring Codex tasks stored
on this machine. It launches a short-lived `codex app-server --stdio` process
and uses the documented JSON-RPC API; it never reads Codex's JSONL logs or
SQLite database directly.

```sh
conversations doctor
conversations list
conversations show THREAD_ID
conversations activity SESSION_OR_THREAD_ID TURN_ID
conversations search 'approved design'
conversations export --json > conversations.json
conversations refresh
```

The default corpus contains interactive root tasks from both active and
archived stores. Pass `--include-subagents` to include spawned tasks or
`--include-exec` to include non-interactive `codex exec` tasks. Message output
contains only normalized user and assistant text and stable
host/thread/turn/item references. Reasoning, tools, approvals, and internal
events are excluded. On macOS the default host reference is derived from an
opaque hash of the platform UUID, so a machine rename does not change it and
the raw hardware identifier is never exposed.

`activity` is a separate, opt-in completed-turn projection for local
automation. It reports normalized message counts plus stable item references
and counts for successfully completed file-change items. It never reports file
paths, diffs, commands, tool output, approvals, or reasoning. A session hint is
validated as an exact thread first; only an exact miss searches visible members
of the same App Server session lineage, and multiple matching threads fail.

Embedded Rust callers can use
`AppServerClient::read_thread_summary(&ThreadRef)` to retrieve exact persisted
task metadata, including the recorded working directory, for a canonical
machine-local thread reference. The lookup remains App Server-only,
state-database-only, and metadata-only; it rejects references for another host.

`export --json`, `show --json`, and search output can contain private transcript
text. Treat redirected files and terminal history accordingly.
Interactive commands inherit App Server diagnostics by default. Embedded or
scheduled callers with private-log requirements should set
`ClientConfig.stderr_policy` to `StderrPolicy::Suppress`.

See [CLI behavior](docs/cli.md), [architecture](docs/architecture.md), and
[macOS installation](docs/system-installation.md).

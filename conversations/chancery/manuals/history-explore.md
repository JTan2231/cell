# Explore local Codex history

Use Conversations when the requested source is Codex task history stored on
this machine. Start with `conversations doctor` when CLI/App Server readiness or
version compatibility is unknown, then select the narrowest operation:

- `list` for metadata;
- `show` for one task;
- `activity SESSION_OR_THREAD_ID TURN_ID` for content-free metadata about one
  completed turn;
- `search` for title plus client-side message matching;
- `export --json` for a typed deduplicated corpus; or
- `refresh` only when App Server metadata scan-and-repair is intended.

An embedded Rust product that already has a canonical machine-local
`ThreadRef` can call `AppServerClient::read_thread_summary(&ThreadRef)` for the
exact persisted `ThreadSummary`, including App Server's recorded `cwd`. The
reference host must match the client's stable host identity. This lookup reads
active and archived state-database metadata with every source kind enabled; it
does not load turns or expose the working directory through `activity`.

Default operations include active and archived interactive root tasks and ask
App Server to use its state database only. Add `--include-subagents` only when
spawned tasks are in scope, and `--include-exec` only for non-interactive runs.
Use `--updated-after`, search `--thread-limit`, or export `--limit` before
full-text work on a large history. The apparent runtime status belongs to the
new App Server process and must not be interpreted as machine-wide liveness.

The interactive CLI inherits App Server stderr. An embedded or scheduled caller
whose logs must remain body-free should use the Rust library with
`ClientConfig.stderr_policy = StderrPolicy::Suppress`; protocol errors remain
observable through the returned result.

Each Unix invocation owns a private process group for the selected Codex
command and any inherited App Server descendants. Conversations terminates
that group when the operation ends, including after an error, without selecting
unrelated Codex processes.

The default macOS host reference is an opaque hash of the platform UUID rather
than the mutable hostname. The raw hardware UUID is not exposed or retained;
use `CONVERSATIONS_HOST_ID` only when an explicit stable override is required.
If macOS cannot read its platform identity, Conversations fails rather than
silently changing the host component.

Show, search, and export expose user and assistant text, which can be private.
Never send or publish that output without separate authority. A failed page or
unsupported full-history record fails the operation.

Empty and whitespace-only user or assistant text is valid and preserved. A user
item with no text parts has empty text. Turns with no normalized messages are
retained. Empty content does not fail history or activity reads. Activity counts
message records, including empty messages; interpretation belongs to the caller.

Activity returns metadata without transcript content. It emits only turn timing/status, message
counts, stable references, and counts of structurally validated completed file
changes. It never emits transcript bodies, file paths, diffs, commands, tool
output, approvals, or reasoning. Session hints are exact-thread-first; lineage
fallback must find exactly one thread containing the requested turn.

## Output selection

CLI list and search use output schema 2 and include `has_more`. The default
`--limit` is 20; supply a positive integer to change it. List rows contain a
reference, title, archive flag, update time, source, and runtime status.

Search returns one thread hit per title match and separate message hits. Each
message hit has a stable reference, title, role, and marked excerpt of at most
240 Unicode characters. Search also reports the optional `thread_limit` scope.

Show and export return complete normalized user and assistant transcripts.
`--json` changes only the encoding. Activity returns metadata without content;
refresh returns metadata counts. Rust metadata methods return complete
`ThreadSummary` values.

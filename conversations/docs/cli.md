# CLI

All commands accept `--codex PATH` (or `CONVERSATIONS_CODEX`) and an optional
stable `--host-id` (or `CONVERSATIONS_HOST_ID`). Without an override, macOS
uses an opaque hash of the platform UUID; the raw hardware identifier is never
returned or retained. macOS fails closed if that stable identity cannot be
read; other platforms use hostname as a compatibility fallback. `--json` uses
camel-case fields and stable typed references.

The CLI explicitly defaults `--app-server-stderr inherit`, preserving Codex
diagnostics for an interactive operator. `--app-server-stderr suppress` routes
only the spawned App Server's diagnostic stream to the null device. JSON-RPC
errors still fail and are reported by Conversations itself.

Every command is short-lived. On Unix its selected Codex command and inherited
App Server descendants run in one private process group that Conversations
terminates on return or error; unrelated Codex desktop and CLI processes are
not selected by that cleanup.

## `conversations doctor [--json]`

Verifies that the selected Codex binary can start and complete an App Server
handshake, enumerates visible root tasks without storage repair, reports the
App Server user agent when available, and warns about recorded CLI-version
differences. Its runtime-status warning is intentional: `notLoaded` only
describes this new App Server process and is not proof that another client is
idle.

## `conversations list [FILTERS] [--title TEXT] [--limit N] [--json]`

Lists task metadata without loading message content. Filters are:

- `--archive active|archived|all` (default `all`);
- `--include-subagents` (default root tasks only);
- `--include-exec` (default interactive tasks only);
- `--cwd PATH` for App Server's exact recorded working-directory filter; and
- `--updated-after UNIX_SECONDS` before any full-history read; and
- `--title TEXT` for App Server's case-sensitive extracted-title search.

Human output is a stable tab-separated table. JSON returns `ThreadSummary`
objects.

## `conversations show THREAD_ID [--turn TURN_ID] [--json]`

Shows one normalized transcript, optionally restricted to a turn. JSON and
human output contain user/assistant text only. They never contain reasoning,
tool calls/results, command payloads, approvals, or internal App Server items.

## `conversations activity SESSION_OR_THREAD_ID TURN_ID [--json]`

Reports content-free metadata for one completed turn: its stable host, thread,
and turn reference; start and completion timestamps; status; user and assistant
message counts; and stable item references plus change counts for completed,
nonempty file-change items. It never prints transcript text, file paths, diffs,
commands, tool output, approvals, or reasoning.

The first identifier can be an exact App Server thread ID or a session hint
from a Codex hook. Conversations validates it as an exact thread first. On an
exact miss it searches active and archived members of the same App Server
session/parent/fork lineage and requires the turn ID to identify exactly one
thread. No match exits nonzero as not-found; copied matches across multiple
threads exit nonzero as ambiguous. The lookup is state-database-only and never
repairs App Server metadata.

## `conversations search QUERY [FILTERS] [--thread-limit N] [--limit N] [--json]`

Search has two parts: App Server searches extracted titles, while Conversations
separately loads the selected normalized corpus and searches message text
case-insensitively on the client. App Server title search is not a full-text
index. `--thread-limit` caps the newest candidate summaries before histories
are loaded; `--limit` caps matching messages afterward. Results are
deduplicated by item ID across copied fork history. Without an updated-after or
thread limit, full-text search must read every selected history and can be
expensive on a machine with many tasks.

## `conversations export [FILTERS] [--limit N] [--json]`

Materializes the filtered, fork-deduplicated normalized corpus. Human output is
a count summary; `--json` writes the full typed corpus to standard output. The
JSON is user/assistant-only but still contains sensitive transcript text and
should be redirected only to an appropriately protected destination. `--limit`
caps the newest selected summaries before any full-history read. Large unbounded
exports necessarily read every selected task.

## `conversations refresh [--json]`

Explicitly enumerates active and archived stores with App Server's metadata
scan-and-repair behavior enabled. It reports counts and does not load message
content. Every other command uses state-database-only listing.

An App Server protocol, pagination, timeout, or history-compatibility failure
exits nonzero. Commands never fall back to raw Codex storage.

# CLI

All commands accept `--codex PATH` (or `CONVERSATIONS_CODEX`) and an optional
stable `--host-id` (or `CONVERSATIONS_HOST_ID`). Without an override, macOS
uses an opaque hash of the platform UUID; the raw hardware identifier is never
returned or retained. The command fails on macOS if it cannot read that stable
identity. Other platforms use the hostname as a compatibility fallback.
`--json` preserves stable typed references. The sections below describe each
command's output.

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

List returns up to 20 rows by default. JSON schema 2 returns `threads` and `has_more`;
rows contain stable reference, title, archive flag, update time, source kind
and observed runtime status. Human output carries the same selection.
To read more, set `--limit` to a larger positive integer. Library metadata methods return
complete `ThreadSummary` values.

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
are loaded; positive `--limit` defaults to 20 hits afterward. Each title match
emits one `kind: thread` hit. Matching messages emit `kind: message`, stable
item reference, thread title, role and a marked `excerpt` of at most 240 Unicode
characters around the match. Message hits are deduplicated by item ID across
copied fork history. JSON schema 2 includes `hits`, `has_more` and the explicit
`thread_limit` search scope. Increase `--limit` for more hits; a result cap
never turns an incomplete source read into success. Without an updated-after or
thread limit, full-text search must read every selected history and can be
expensive on a machine with many tasks.

## `conversations export [FILTERS] [--limit N] [--json]`

Materializes the filtered, fork-deduplicated normalized corpus. Text renders
the complete selected transcripts; `--json` encodes the same full corpus. The
corpus is user/assistant-only but still contains sensitive transcript text and
should be redirected only to an appropriately protected destination. `--limit`
caps the newest selected summaries before any full-history read. Large unbounded
exports necessarily read every selected task.

## `conversations refresh [--json]`

Enumerates active and archived stores and allows App Server to scan and repair
its metadata. It reports counts and does not load message
content. Every other command uses state-database-only listing.

An App Server protocol, pagination, timeout, or history-compatibility failure
exits nonzero. Commands never fall back to raw Codex storage.

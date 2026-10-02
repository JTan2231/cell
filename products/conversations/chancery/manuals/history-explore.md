# Explore local Codex history

Use Conversations to retrieve local Codex task metadata, normalized user and
assistant history, or completed-turn activity. Codex App Server owns discovery,
stored metadata, pagination, source records, and storage compatibility.
Conversations owns selection, stable references, normalization, deduplication,
and explicit failure when requested evidence cannot be read completely.

Read `conversations.runtime` for executable and host selection, diagnostic
privacy, private process cleanup, readiness, and installation guarantees.
Start with `conversations doctor` when readiness or compatibility is unknown.
The Rust library is the reusable typed boundary; local products should use it
instead of scraping human CLI output.

## Select records and interpret metadata

The default corpus contains active and archived interactive root tasks.
`--include-subagents` and `--include-exec` include those sets independently.
Conversations sends every documented source kind to App Server: `cli`, `vscode`,
`exec`, `appServer`, `subAgent`, `subAgentReview`, `subAgentCompact`,
`subAgentThreadSpawn`, `subAgentOther`, and `unknown`. By default it excludes all
`subAgent*` kinds, records with `parentThreadId`, and the `exec` source.

Active and archived metadata require separate complete paginated reads.
Ordinary commands send `useStateDbOnly: true`; observation cannot trigger the
App Server scan-and-repair path. `refresh` is the sole explicit exception.
Runtime status describes only the newly launched App Server and is not
machine-wide liveness. No Conversations cache is consulted.

Common filters are `--archive active|archived|all` (default `all`),
`--include-subagents`, `--include-exec`, `--cwd PATH` for App Server's exact
recorded working directory, and `--updated-after UNIX_SECONDS`. Updated-after
and candidate-thread limits select sorted summaries before full-history reads.
Use those bounds before full-text work on a large history.

## Stable references and normalized data

`ThreadRef` contains host and thread ID. `TurnRef` adds turn ID. `ItemRef`
adds item ID. Typed `ThreadSummary`, `Turn`, and `Message` records surround
those references. Read `conversations.runtime` before choosing a host override.

Only `userMessage` text content and `agentMessage` text become `Message`
records. Images, reasoning, commands, tool calls or results, approvals, plans,
and other internal items are omitted. Empty and whitespace-only text is valid
and preserved. A user item with no text parts has empty text. Turns with no
normalized messages are retained. Callers own interpretation of those values.

A message uses its item timestamp when available. Otherwise it uses the turn's
`startedAt` with `timestampPrecision: turn`, or has unknown precision when no
timestamp is available. `--updated-after` uses Unix seconds. Activity counts
message records and completed change entries, including empty message records.

Full history prefers ascending `thread/turns/list` with `itemsView: full`
and complete cursor pagination. Only exact method-unavailable code `-32601`
permits legacy `thread/read` with `includeTurns: true`. Other errors never
trigger fallback or private-storage access.

Forks can copy item IDs. `snapshot`, `search`, and `export` keep an item ID
once and assign it to the most recently updated enumerated task.
`show THREAD_ID` is a view of that task and deduplicates repeated IDs within it.
Full-text cost grows with the selected complete histories; there is no persistent
index. Metadata-only `list` and `doctor` do not load turn content.

## List metadata and resolve an exact summary

```sh
conversations list [FILTERS] [--title TEXT] [--limit N] [--json]
```

List returns up to 20 rows by default. `--limit` accepts a larger positive
integer. JSON output schema 2 contains `threads` and `has_more`; rows contain
reference, title, archive flag, update time, source kind, and observed runtime
status. Human output carries the same selection. `--title TEXT` uses App
Server's case-sensitive extracted-title search. Rust metadata methods return
complete `ThreadSummary` values.

An embedded caller with a canonical machine-local `ThreadRef` can call
`AppServerClient::read_thread_summary(&ThreadRef)`. The host must match the
client's stable identity. The lookup fully enumerates active and archived
state-database metadata with all source kinds enabled and requires the thread
ID to occur exactly once. It returns the persisted `ThreadSummary`, including
recorded `cwd` and owning archive, without loading turns or changing CLI or
activity output. A foreign host, missing or duplicate thread, or incomplete
page causes an error.

## Show, search, and export history

```sh
conversations show THREAD_ID [--turn TURN_ID] [--json]
conversations search QUERY [FILTERS] [--thread-limit N] [--limit N] [--json]
conversations export [FILTERS] [--limit N] [--json]
```

Show returns one normalized transcript, optionally restricted to a turn.
Search combines App Server's case-sensitive extracted-title query with separate
case-insensitive client-side message matching over the selected corpus. Title
search is not a transcript index. Export returns complete selected transcripts
with fork-copy deduplication. `--json` changes the encoding, not the content scope.

Search returns one `kind: thread` hit per title match and separate
`kind: message` hits with stable item reference, thread title, role, and a
marked excerpt of at most 240 Unicode characters. Positive `--limit` defaults
to 20 hits after retrieval. Schema 2 contains `hits`, `has_more`, and explicit
`thread_limit` scope. `--thread-limit` caps newest candidate summaries before
histories are loaded. Export `--limit` caps newest selected summaries before
any full-history read. Without those bounds, every selected history is read.
A result cap never converts an incomplete source read into success.

## Inspect completed-turn activity

```sh
conversations activity SESSION_OR_THREAD_ID TURN_ID [--json]
```

Activity is an opt-in projection. It adds no fields to `Conversation`, `Turn`,
`Message`, show, search, or export. Rust `TurnActivity` contains the existing
normalized `Turn` and `CompletedFileChange` records. CLI activity emits only
stable host/thread/turn reference, timing, status, message counts, completed
file-change item references, and change counts. It emits no transcript text,
paths, diffs, commands, tool output, approvals, or reasoning.

An eligible turn has status `completed` and a `completedAt` timestamp. Every
file-change item and documented `{path, diff, kind}` entry is structurally
validated. Only stable item reference and change count are retained. Paths,
diffs, move destinations, and other payloads are discarded. Failed, declined,
in-progress, and empty file changes are excluded. Unknown statuses or malformed
entries fail the read. A message-free turn has zero message counts; empty
messages still count as retained message records.

The first identifier is tested as an exact thread ID. Only an exact miss
lists active and archived metadata and searches App Server `sessionId`,
`parentThreadId`, and `forkedFromId` lineage. The requested turn must occur
in exactly one candidate: no match is not-found and multiple copied matches
are ambiguous. The lookup does not repair metadata. Exact activity uses
newest-first full-turn pages of 100 and stops after the requested turn's page.
Legacy fallback remains limited to `-32601`.

`read_completed_turn_activities` lets a caller reconcile a missed event. It
fully paginates one selected task and returns each completed `TurnActivity`.
Interrupted, failed, and currently running turns are ineligible. Conversations
does not listen for events or retain an index, daemon, or reconciliation state.

## Refresh metadata deliberately

```sh
conversations refresh [--json]
```

Refresh enumerates active and archived stores with `useStateDbOnly: false`,
allowing App Server to scan and repair its metadata. It returns metadata counts
without loading message content. Run it only when that repair is intended;
every other command uses state-database-only listing.

## Completeness, failure, and recovery

A successful result completes every requested metadata page and selected
full-history page, or the documented method-unavailable fallback. A protocol
error, timeout, unreadable page, unsupported full-history record, or ambiguous
activity lookup stops the operation. Conversations does not return an incomplete
normal corpus. The App Server view is read at invocation time; it is not a
promise that metadata reflects other clients' liveness or remains unchanged.

Inspect `conversations doctor` and the selected executable on compatibility
failure. Use refresh only for intended metadata repair. Stop when required data
is unavailable through App Server. Do not read raw storage, mutate tasks, change
authentication, or infer missing evidence from unrelated processes.

No App Server availability, latency, corpus-size, throughput, perpetual
history-retention, future source-kind, protocol-support, or deprecation window
is promised. Provider and entry contract versions are distinct from Codex and
App Server versions. The upstream data surface has no dedicated installed
Chancery contract; `resolve` preserves that reliance gap.

## Privacy and authority

Titles, recorded working directories, stable references, activity counts, and
normalized user and assistant text can be private. Show, search, and export
write sensitive transcript text to the caller's output. Select a protected
destination for retained exports. Read `conversations.runtime` before allowing
App Server diagnostics into embedded or scheduled logs.

Conversations sends no transcript to a model or network service of its own.
This contract does not authorize disclosure or publication, model interpretation,
task continuation, mutation, archive, deletion, messaging, reasoning or tool
extraction, raw storage access, or machine-wide liveness monitoring.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

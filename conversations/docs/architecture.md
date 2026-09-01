# Architecture

## Product boundary

Conversations is a short-lived local adapter over the documented
[Codex App Server](https://learn.chatgpt.com/docs/app-server). It owns stable
query behavior and a normalized transcript schema. App Server owns discovery,
storage compatibility, pagination, and stored task metadata. Conversations has
no daemon, database, authentication flow, model call, scheduler, or network
service, and it never opens Codex JSONL or SQLite files.

The Rust library is the reusable boundary. The CLI only parses arguments and
renders the library's typed values. Other Cell products should depend on the
library instead of scraping human CLI output.

## Connection and reads

Each invocation starts `codex app-server --stdio`, sends one `initialize`
request with `experimentalApi: true`, sends `initialized`, performs the bounded
operation, and terminates the launch. On Unix, Conversations starts the command
as the leader of a private process group and kills that group before reaping
the direct child. This keeps a CLI wrapper's App Server descendants from
surviving the invocation while leaving every unrelated Codex process outside
that group untouched. The unreaped direct child pins the process-group identity
until it has been signaled, preventing PID reuse from redirecting cleanup.
Requests use newline-delimited JSON-RPC messages with the wire shape documented
by App Server.

`ClientConfig::stderr_policy` makes the diagnostic boundary explicit.
`Inherit` is the library and interactive CLI default so operators can see Codex
startup and compatibility diagnostics. Privacy-sensitive embedded callers can
select `Suppress`, which connects App Server stderr to the null device before
spawn. Scheduled callers that promise body-free logs must select `Suppress`;
this changes diagnostics only and never changes JSON-RPC completeness checks.

Every `thread/list` request sends the complete documented source-kind set:
`cli`, `vscode`, `exec`, `appServer`, `subAgent`, `subAgentReview`,
`subAgentCompact`, `subAgentThreadSpawn`, `subAgentOther`, and `unknown`.
Active and archived tasks require separate paginated calls. The default then
filters out every `subAgent*` source kind and every record with a
`parentThreadId`, including older records that lack a parent, and filters out
the `exec` source. `--include-subagents` and `--include-exec` opt those sets in
independently.

Ordinary commands send `useStateDbOnly: true`, so observation cannot trigger
App Server's log scan-and-repair path. `refresh` is the single explicit
exception: it enumerates active and archived stores with
`useStateDbOnly: false`, allowing App Server to repair its own metadata.

Embedded callers can resolve one exact canonical `ThreadRef` with
`AppServerClient::read_thread_summary`. The reference's host must match the
client's configured stable host identity. The lookup enumerates App Server's
state-database-only active and archived metadata with every source kind enabled,
then requires the thread ID to occur exactly once. It returns the existing
`ThreadSummary`, including App Server's persisted `cwd` and the owning archive,
without loading turns or changing the CLI's output surface. A foreign host,
missing thread, duplicate thread, or incomplete page is an error rather than an
attribution guess.

Full history prefers experimental `thread/turns/list`, ascending, with
`itemsView: full` and cursor pagination. If the installed App Server reports
that method as unavailable, Conversations falls back to legacy `thread/read`
with `includeTurns: true`. Other failures remain failures. In particular, a
future paginated record that App Server cannot fully read is not guessed from
private storage.

Exact activity reads use the same full-turn method newest-first in pages of
100, stopping after the page containing the requested turn. A hook-provided
session hint is first tested as an exact thread ID. Only an exact miss lists
active and archived metadata and searches threads connected by App Server
`sessionId`, `parentThreadId`, or `forkedFromId`; the requested turn must occur
in exactly one candidate. This matters because forks can share a session ID and
can copy older turn IDs. Zero matches is not-found and multiple matches is an
ambiguity error. The legacy full-thread fallback remains limited to exact
method-unavailable code `-32601`.

## Normalized corpus

The public corpus has four stable reference levels:

- `ThreadRef`: host and thread ID;
- `TurnRef`: host, thread, and turn ID;
- `ItemRef`: host, thread, turn, and item ID; and
- the surrounding typed `ThreadSummary`, `Turn`, and `Message` records.

On macOS the default host component is an opaque truncated SHA-256 digest of
the platform UUID, so renaming the Mac does not churn references and the raw
hardware UUID is never exposed or retained. `CONVERSATIONS_HOST_ID` remains an
explicit override. A macOS read failure stops the operation rather than
silently switching identity; hostname is only a non-macOS compatibility
fallback.

Only `userMessage` text content and `agentMessage` text become `Message`
records. Images, reasoning, command execution, tool calls/results, approvals,
plans, and other internal items are omitted rather than partially exposed.
App Server currently timestamps turns but not every item, so a message uses an
item timestamp when one exists, otherwise its containing turn's `startedAt`
with `timestampPrecision: turn`, otherwise `unknown`.

## Completed-turn activity

Activity is a separate opt-in projection and does not add fields to
`Conversation`, `Turn`, `Message`, `show`, `search`, or `export`. A
`TurnActivity` contains the existing normalized `Turn` plus
`CompletedFileChange` records. An eligible turn must report status `completed`
and a `completedAt` timestamp. Each file-change item and each documented
`{path, diff, kind}` change entry is structurally validated, but only the
stable item reference and number of changes are retained. Paths, diffs, move
destinations, commands, tool output, approvals, and reasoning are discarded.
Failed, declined, in-progress, and empty file changes do not become completed
file-change evidence. Unknown statuses or malformed entries fail the activity
read instead of becoming a false negative or false positive.

`read_completed_turn_activities` is the reconciliation path for a caller that
missed an event. It fully paginates one selected task and returns every
completed `TurnActivity`; interrupted, failed, and currently running turns are
not eligible. This remains a short-lived read. Conversations does not listen
for events, keep an index, or run a daemon.

Forks can copy item IDs. `snapshot`, `search`, and `export` keep each item ID
once, assigning it to the most recently updated enumerated task. This prevents
copied history from being counted repeatedly. `show THREAD_ID` remains a view
of that selected task and deduplicates repeated IDs inside it only.

Full-text work is proportional to the selected full histories. Updated-after
and candidate-thread limits are applied to sorted summaries before `snapshot`,
`search`, or `export` performs any full-turn read. Metadata-only `list` and
`doctor` never load turn content.

## Search and liveness

App Server `searchTerm` searches extracted titles and is case-sensitive. The
`search` command makes that title query, then separately materializes the
selected corpus and performs case-insensitive client-side full-text matching.
These are deliberately distinct operations; title search is not treated as a
transcript index.

Runtime status is scoped to the new App Server process. A persisted task often
appears as `notLoaded` even when another Codex client owns a live process.
Conversations reports the observed value as `runtimeStatus` and does not inspect
processes or claim machine-wide liveness. `doctor` also compares the launched
CLI version with versions recorded on visible tasks, because a separately
installed CLI can lag the Codex desktop build that wrote them.

## Privacy and failure

List metadata can reveal titles and working directories. Show, search, and
export can reveal complete normalized user and assistant transcript text.
Output goes only to the caller's standard streams; redirected output is the
caller's file and security responsibility. A protocol error, unreadable page,
timeout, or unsupported full-history format stops the operation rather than
returning a silently incomplete corpus.

By default, App Server diagnostics also inherit the caller's stderr and can
contain paths or other operational context. Use the explicit suppression policy
for embedded or scheduled contexts whose log privacy contract cannot retain
those diagnostics.

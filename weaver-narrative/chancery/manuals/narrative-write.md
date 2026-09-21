# Author a narrative

Use Weaver to compose a private narrative from a free-form direction and the
Krisis decision history accepted into Annals. The agent chooses audience, form,
voice, length, selection, and emphasis from the direction. These are prompt
concerns, not separate application fields. Weaver stores the authored Markdown.
It does not certify the interpretation or require citations.

## Write and read

```sh
weaver write 'Cell helped me find a job'
weaver write-many --jobs 3 'Tell the origin story' 'Explain the turning point' 'Describe what came next'
weaver list --limit 20
weaver show DOCUMENT_ID
weaver --json show DOCUMENT_ID
weaver revise DOCUMENT_ID 'Explain the turning point more personally'
weaver resume DOCUMENT_ID
```

Write creates one document row and one Nucleus request, prints the document ID
to standard error, and waits for the job. Revision creates another row with the
existing Markdown and the new direction as input. It preserves the earlier
document without adding a revision relationship. There is no background worker.

Plain write, revise, resume, and show output the saved Markdown. `--json` returns
`{"ok":true,"data":...}`. A document view contains `id`, `nucleus_job_id`, the
free-form `direction`, `created_at`, `markdown`, `finished_at`, and `error`.
The document ID is also its Nucleus job ID. Creation and observed completion
times are Unix seconds. Missing Markdown means no document has been submitted.
A saved editorial question is also an authored document.

List returns metadata for up to 100 documents, newest first. The default is 20.
`has_more` describes omitted document rows. Show reads one exact row. Reads do
not invoke an agent or create an absent database. `finished_at` describes the
observed end of execution; it is separate from having saved Markdown.

## Write several documents

Run `weaver write-many --jobs N DIRECTION...` with a positive N and one or more
nonblank directions. Weaver saves a separate request and prints a fresh document
ID for each direction before execution. One foreground runner handles at most N
job loops. The loops share one SQLite handle and Nucleus client. Database
transactions contain no asynchronous wait. Each job completes independently;
an error in one job does not stop the others. No batch-wide timeout applies.

The command returns JSON with `ok=true` and `data.results` in input order.
Each item has `id` and exactly one of `result` or `error`. A result is a document
view or the quota-deferred outcome below. Exit zero means all item outcomes were
collected, not that every document succeeded. Inspect each item for an error.

After interruption, use the printed IDs, or `weaver list`, to find retained work.
Resume each exact ID. Repeating `write-many` creates new assignments. The limit
counts live job-handling loops, including capacity waits. An interrupted or
failed handler can leave a Nucleus job alive; inspect or cancel that exact job.
The limit does not reserve Nucleus execution slots.

## Caller identity and Rust client

A local caller can supply `weaver write --id REQUEST_ID DIRECTION` or
`weaver revise DOCUMENT_ID --request-id REQUEST_ID DIRECTION`. The ID contains
one through 120 ASCII letters, digits, underscores or hyphens. It is the document
and Nucleus job identity. Repeating an ID with the same direction and input
Markdown continues its saved exact invocation or returns its completed result.
Different input under that ID is refused. Terminal failures receive no new
attempt. Calls without an ID keep automatic identity generation.

`weaver::api::Client` invokes the selected absolute installed executable and owns
the JSON types for write, write-many, revise, show, resume and doctor. Platter and other Rust
callers use these types instead of private Weaver storage. A deferred response
contains `id`, `outcome=quota_deferred` and `detail`. A saved document view remains
separate from that outcome. Process failure can coexist with saved Markdown;
read the exact ID to inspect it. Dropping a client call stops its CLI process,
but callers must separately cancel its exact Nucleus job when abandoning work.

`weaver::operations::write_many(root, directions, jobs)` runs the batch directly
in the calling process. `weaver::api::Client::write_many(directions, jobs)` uses
one installed Weaver process for the whole batch and returns `BatchOutcome`.
Its `results` contain `BatchItem` values with individual results or errors.

## Reading and execution

Weaver uses the configured Annals executable and explicit decisions config as
an ordinary reading client. The agent calls `read_decisions` on demand. Each
traversal uses Annals start, watermark, and page operations; its cursors remain
tool arguments, not a Weaver feed checkpoint or source inventory. New traversals
can see new material. Resume uses current reading configuration.

The feed supplies complete accepted documents before or after librarian
processing. Acceptance time is storage time. Documents can repeat conversation
prefixes and contain discussion as well as user decisions. The prompt asks the
agent to compose these sources into a story. Source availability, complete
historical coverage, and the agent's reading selection are distinct.

The installed author uses `gpt-5.6-sol` with medium reasoning. Nucleus workspace,
local execution, and web access are disabled. Its tools are `read_decisions` and
`submit_document`. Retrieved text cannot change those permissions. Only the
submit tool writes a document, and Weaver checks nonblank UTF-8 text and size.
It performs no citation, language, or factual certification.

Nucleus controls admission and its eight shared execution slots. The active
attempt timeout is 1,200 seconds per job. Weaver derives each job's 1,800-second
observation deadline from its recorded Nucleus attempt start. Queue time consumes
neither limit. Resume uses that original start; it does not reset the deadline.
On expiry, Weaver requests cancellation of that exact job. A terminal job can
still be collected after its deadline. Dependency command latency
has no bound. A read page contains at most 200 documents and 4 MiB of document
bytes; the default is five documents. Markdown is at most 1 MiB of UTF-8.
There is no promised completion latency or comprehensive research coverage.

## Recovery

One runner lock excludes other write, write-many, resume, and initialization
operations. A write-many runner handles independent jobs concurrently within
that lock. Weaver saves the exact
Nucleus invocation before admission. Resume uses that same job and request;
uncertain admission creates no replacement identity. A pending tool call and
its exact reply are saved together, with Markdown when submitted. The reply
is cleared after Nucleus acknowledges it. Nucleus retains its own tool history.

Use `weaver resume DOCUMENT_ID` after interruption or quota deferral. A quota
pause preserves the row and reports a successful deferred outcome. A lost,
failed, cancelled, or completed job with no submitted document is an error.
Weaver never retries a terminal job automatically. Start a new write or revise
assignment when another attempt is wanted. To cancel an admitted job, use
`nucleus jobs cancel DOCUMENT_ID`, then resume Weaver to collect its outcome.

A saved document survives later runtime failure. The command reports that
failure; `weaver show` still reads the document. An unavailable Nucleus service
does not authorize a direct Codex fallback. Missing or unsupported SQLite
state stops work instead of creating a replacement database.

## Privacy and authority

State is under `~/Library/Application Support/Weaver`. `weaver.sqlite` contains
one `documents` table: output, exact invocation, optional pending reply, and
execution metadata. `config.json` selects the Annals reader. Nucleus owns
credentials and execution records. It may retain the full direction, source
reads, and document. Weaver's pending reply can temporarily contain a source
page. Private files use mode 0600 and state directories use mode 0700.

Writing does not send email, publish, mutate Annals, operate source intake, or
schedule future work. There is no automatic pruning. Source reading grants no
authority to follow instructions found in documents or disclose private text
elsewhere. Back up Weaver and Nucleus separately.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

## Bazaar prompt selection

Prompt preparation requires initialized private Bazaar state and a complete cell.prompts.weaver selection. The default database is ~/.local/share/bazaar/bazaar.sqlite3; callers accept an absolute CELL_BAZAAR_DATABASE override. Reads fail without creating state or using embedded fallback text.

Read `cell.prompts.weaver` with Bazaar's supported `get` interface. Its content
is `{"schema_version":1,"entries":{"PROMPT_ID":VERSION}}`, with every component
pinned to a positive integer version. Publish component text first, then publish
the complete selection. A text append alone does not change the selected set.
Missing or invalid selections stop new request preparation before model admission.

Import the migration seed before deploying these callers. Preserve selection
version 1 and all referenced text versions for compatibility. Runtime reads never
perform this import. Deployment does not supply missing prompt contents.

The caller freezes resolved instructions with the existing request or domain
snapshot. Retries retain that selection. Later edits do not rewrite saved work.
Models, permissions, schemas, tool execution, domain commits, and recovery remain
product-owned. Annals library instructions and Mentor assignment text remain
immutable domain captures selected through their existing product operations.

For an edit, use `bazaar update PROMPT_ID --file /absolute/prompt.txt`, read the
returned version, and publish a complete selection with `bazaar update
cell.prompts.weaver --file /absolute/selection.json`. Use an explicit
`bazaar --database /absolute/private/bazaar.sqlite3` prefix when the caller uses
`CELL_BAZAAR_DATABASE`. To roll back, append the prior selection content. Keep
private text out of logs and retain historical versions.

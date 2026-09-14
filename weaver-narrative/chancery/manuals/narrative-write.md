# Author a narrative

Use Weaver to compose a private narrative from a free-form direction and the
Krisis decision history accepted into Annals. The agent chooses audience, form,
voice, length, selection, and emphasis from the direction. These are prompt
concerns, not separate application fields. Weaver stores the authored Markdown.
It does not certify the interpretation or require citations.

## Write and read

```sh
weaver write 'Cell helped me find a job'
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
attempt timeout is 1,200 seconds. Weaver observes the operation for up to 1,800
seconds and requests cancellation on that deadline. Dependency command latency
has no bound. A read page contains at most 200 documents and 4 MiB of document
bytes; the default is five documents. Markdown is at most 1 MiB of UTF-8.
There is no promised completion latency or comprehensive research coverage.

## Recovery

One runner lock serializes write and resume operations. Weaver saves the exact
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

CLI dispatch attempts metadata-only Chancery usage recording. It records the
command identity and optional task correlation, not arguments or output.
Recording failure preserves the product result. `--register-usage` separately
registers the command inventory without writing a narrative.

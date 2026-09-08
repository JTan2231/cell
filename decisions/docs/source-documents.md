# Decision source documents

The observer and `krisis document` share one builder for classifying a completed
exchange and constructing a Markdown source document. The observer records
coverage and delivers through its durable Annals outbox. Explicit local
`document` commands only construct files; they do not write observer coverage,
an Annals outbox, or consumer cursors.

## Build and resume

Use the built `krisis` executable and a separate directory for each exchange:

```sh
krisis document build --thread-id THREAD --turn-id TURN --directory DIRECTORY
```

The directory is created with mode `0700`. An existing directory must be private.
Krisis serializes commands for that directory with a file lock. The normal Krisis
deployment admission gate also applies. Annals configuration is not required,
and the command does not open or migrate the observer database.

Conversations supplies the complete normalized history of an exact root
interactive task. Krisis freezes all turns through the selected completed turn,
including all user and assistant messages in their supplied order. The selected
turn must contain a nonblank user message. Missing completion or inconsistent
source identities cause an error; they do not produce a negative verdict.
Earlier messages can have unknown timestamps. Later turns are excluded.

“Complete history” means Conversations' normalized user/assistant text. It does
not include tools, reasoning, file changes, or attachment binaries that this
interface does not expose. Text is not paraphrased, filtered, or truncated.

The classifier sees every captured message and which final exchange to judge.
Its encoded JSON prompt can contain at most 262,144 UTF-8 bytes. Larger input
fails before Nucleus submission. There is no message-count limit or automatic
context reduction.

## Classification and rendering

The agent answers whether the final exchange contains a user decision. Earlier
exchanges provide context. A decision is an explicit user settlement constraining
intended behavior or state. The complete generated result is:

```json
{"is_decision": true, "summary": "The user chose the proposed approach and deferred delivery integration."}
```

For a negative verdict, `is_decision` is `false` and `summary` is `null`.
The agent is instructed to write one or two sentences for a positive verdict.
Code requires a nonblank, single-line summary of at most 1,000 Unicode scalar
values. Sentence counting and the meaning of the prose are not validators.
Unknown fields, missing fields, wrong types, and inconsistent verdict/summary
shape are rejected with a specific `invalid_structure` explanation. Paths,
identifiers, copied source fragments, and privacy-related words are accepted.

For a positive verdict, code writes `decision.md`. The summary is its first
Markdown heading. Code escapes heading punctuation, then renders every captured
exchange with fixed role headings and source messages in Markdown blockquotes.
Message text is retained, including blank lines and code blocks. There are no
generated context, action, result, authority-quote, or `Unknown.` sections.
A negative verdict produces no Markdown file.

The command emits JSON with `job_id`, `classification`, `document`, and `run`.
`document` is the absolute Markdown path or `null`; `run` is the absolute
`run.json` path. These files are construction results, not Annals receipts or
observer coverage records. The Markdown is an ordinary source document, not a
schema-one decision account.

## Execution and recovery

Nucleus runs `gpt-5.6-terra` with medium reasoning, workspace access `none`,
and no local execution or web search. The new immutable toolset is
`krisis/decision-document/1`. It exposes only `submit_decision`, using input
schema `krisis.tool.submit-decision.input.v1` and result schema
`krisis.tool.submit-decision.result.v1`. Existing account registrations keep
their meaning. No direct Codex fallback exists.

`run.json` contains the frozen source, exact typed Nucleus request, tool-call
digests, exact tool responses, accepted classification, and terminal observation.
Krisis saves the request before submission. It saves each response and any
accepted classification atomically before acknowledging the tool call.
Files use mode `0600`; writes synchronize the file and directory.

Repeat the same build command and directory after an interruption. Krisis reuses
the frozen source and exact request under the same job ID. It does not reread the
conversation or create a successor attempt. Repeated identical calls receive
their saved response; conflicting call content fails. A different exchange
cannot reuse the directory. Back up the entire directory together if needed.
The run contains full conversation text and remains local until removed by its
owner; automatic retention cleanup is not provided.

The agent execution timeout is 20 minutes after Nucleus acquires a slot. The
command waits at most 22 minutes, including queue time. A wait timeout or transport
error leaves the same run resumable. Runtime completion without an accepted
classification is an error. An accepted classification remains valid if the
runtime subsequently fails. A terminal run without a classification is not
automatically retried; inspect its Nucleus job before choosing a new directory.

To reconstruct a document from an accepted result without Nucleus or Conversations:

```sh
krisis document render --directory DIRECTORY
```

Rendering uses only saved state. It can restore a missing `decision.md`, but
refuses to overwrite different existing bytes. It does not acknowledge pending
Nucleus calls; resume the build command to settle those. The same source and
summary always produce the same Markdown bytes.

These local build/resume rules also protect saved observer work. The observer
marks its observation failed on the first returned processing error and does
not resume it automatically. Use `krisis observe retry OBSERVATION_ID` for later
recovery. An uncertain job or saved accepted result reuses the same run; a pending
Annals delivery reuses its existing document. Worker health is reported by
`krisis health` independently of these retained observation failures.

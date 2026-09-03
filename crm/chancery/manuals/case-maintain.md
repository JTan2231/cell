# Maintain a CRM case

Use this capability to create one employment-oriented case, add exact new
information to it, and inspect or explicitly recover the resulting bounded
steward update. Use `crm.library.explore` when the task is read-only search or
case inspection, and `crm.steward.operate` for installation, initialization,
readiness, or operator recovery.

## Boundary

CRM accepts free-form text so one intake line can represent a person, company,
role, job posting, location, introduction, interaction, or outcome without a
large entity model. It stores accepted text in its private SQLite database. An
input path is transport only: CRM does not retain it or create a Markdown file.

CRM does not fetch a source, find an email address, send a message, or contact
anyone. `--source` is an opaque caller-supplied reference. Resolve current
facts through the referenced source before acting on them.

The six case stages are:

```text
research | warranted | contacted | connected | helped | closed
```

They are durable CRM classifications. `warranted` does not authorize contact,
and `connected` or `helped` does not independently prove an external event.

## Create a case

```sh
/Users/joey/.local/bin/crm case new \
  --title "Example hiring lead" notes.md --stage research
```

The optional input is a regular non-symbolic UTF-8 Markdown file or `-` for
standard input, at most 1,048,576 bytes. With no input, CRM uses the title plus
suggested `Current picture`, `People`, `Chronicle`, and `Open threads`
headings. The headings are editorial guidance only: supplied and stewarded
Markdown remains free-form, and CRM does not parse or require them. With no
stage, it uses `research`.

CRM validates the complete input before opening a write transaction. Success
commits the case, retained raw document, and immutable revision one together.
Keep the returned case identity; identifiers are opaque.

## Record new information

```sh
/Users/joey/.local/bin/crm tell CASE_ID update.txt \
  --name "Hiring manager reply" \
  --source "message:2026-09-03"
```

Use `-` to read the required delivery from standard input. The maximum is
1,048,576 UTF-8 bytes. CRM stores the exact text and digest with the optional
label/reference. In the same transaction it creates a queued update. It then
launches the private hidden worker and returns the queued update without waiting
for AI completion; machine output includes both the update and delivery
identities.

The durable tell transaction is the command's success condition. A worker
launch or Nucleus readiness failure after that point cannot erase intake or
turn a successful tell into an absent delivery.

## Inspect progress

```sh
/Users/joey/.local/bin/crm update list --limit 20
/Users/joey/.local/bin/crm update show UPDATE_ID
/Users/joey/.local/bin/crm update wait UPDATE_ID [--timeout SECONDS]
```

`show` distinguishes queued work, an attempt and its Nucleus job, runtime
terminality, accepted tool delivery, and a committed CRM revision. Do not infer
domain success from a completed Nucleus job or final model prose. CRM success
requires the update's committed revision.

`wait` observes until failed/lost work is final or an applied update also has a
retained terminal Nucleus observation; `--timeout SECONDS` defaults to 1,200.
Once on entry, if domain or runtime settlement still needs work, it activates
queue drain or same-update recovery and then only polls. It never creates a
retry. `applied` already means CRM domain success even while that terminal
runtime observation is pending.

## Hidden steward and domain commit

The worker records an attempt before ambiguous submission, freezes the case's
base revision and this delivery, and submits them through Nucleus under:

- requester program `crm`;
- requester identity `case-steward:UPDATE_ID`;
- one unique Nucleus job identity per attempt; and
- immutable toolset `crm/case-steward/1`.

The Codex invocation uses model `gpt-5.6-terra`, medium reasoning, and a
1,200-second timeout. It has workspace access `none`, local execution and web
search disabled, no launch context, and exactly one managed tool:
`submit_case_revision`. The tool supplies the frozen positive base guard plus
one complete replacement Markdown body of at most 1,048,576 bytes, one
supported stage, a nullable nonempty advisory of at most 4,000 bytes, and a
nonempty summary of at most 1,000 bytes.

The steward is shown the same suggested headings but may retain or choose a
better case-specific organization. Heading conformance is never a validation
condition.

CRM rechecks the frozen base while committing. It atomically inserts the next
immutable revision, stores the replay-safe tool receipt and result, advances
the case head, and marks the update applied. After result delivery it retains
the acknowledgment and terminal Nucleus state/detail separately. A stale base
or invalid tool call commits none of those effects.

## Resume and retry

```sh
/Users/joey/.local/bin/crm update resume UPDATE_ID
/Users/joey/.local/bin/crm update retry UPDATE_ID
```

Resume is for queued, exactly recoverable running, or
applied-but-runtime-unsettled work. It processes that update synchronously,
preserving its requester/job identity and any admitted request or pending tool
call. It does not treat uncertainty as permission for a new update.

Retry is allowed only when the selected update is `failed` or `lost`. It reuses
the same immutable delivery row/text in a successor update, records `retry_of`,
and uses new requester/job identities. CRM never retries automatically, and it
has no direct-Codex fallback.

An ambiguous admission can repeat only the exact typed request with the same
job ID. A byte-identical managed-tool redelivery returns the stored result;
conflicting reuse of a call identity fails closed. CRM rechecks exact job
identity and nonterminality before dispatching a pending call. If a domain
commit succeeded before the harness later failed, the update remains applied,
retains the runtime diagnostic, and is not retryable.

## Advisory behavior

Every revision carries a nullable advisory. When present, it appears
conspicuously on case list, search, show and history, tell acknowledgment, and
update list/show/wait/resume/retry. Applied updates use their committed
revision; other updates use their frozen base when assigned and otherwise the
current head. Human output uses
`ATTENTION — STEWARD ADVISORY (NON-BLOCKING)`; JSON carries `attention: true`
and the advisory text. An advisory does not change exit status and never blocks
an otherwise valid case creation, tell, read, stage, resume, retry, or
caller-owned real-world action.

## Privacy and failure

The CRM database and correlated Nucleus state may contain private people,
employment, interaction, source, prompt, advisory, and tool-result text. Use a
private terminal and input path; redirected output becomes caller-owned data.

For invalid input, missing/unsupported storage, stale base, conflicting replay,
or invalid recovery transition, inspect the reported stable identity and error
code. Run `crm doctor` against the same database for integrity/readiness issues.
Do not repair SQLite or Nucleus rows manually.

If `update wait`, `update resume`, or `update retry` fails after resolving the
update, JSON adds a top-level `context` containing that update view and its
relevant attention/advisory; human stderr prints the same nonblocking advisory
banner before the error. The operational failure remains nonzero, but the
advisory is never allowed to disappear merely because processing failed.

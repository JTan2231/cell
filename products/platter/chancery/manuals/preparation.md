# Packet preparation

Use this feature to understand how Platter captures source material, validates
private briefs and resumes, and continues retained preparation. A packet is a
preparation run and its artifacts. Platter owns eligibility, mechanical
acceptance, and domain success. Weaver owns project authoring; Nucleus owns
runtime execution. Preparation supplies no send authority.

Read `platter.materials` for the canonical library and fixed template contract.
Read `platter.editions` for ordinary freshness selection and delivery. Use
`platter.packet.prepare` for procedures and `platter.maintenance` for readiness,
migration, and maintenance.

## Primary interfaces

```sh
platter prepare CAST_JOB_ID
platter prepare-daily
platter prepare CAST_JOB_ID --fresh
platter regenerate CAST_JOB_ID --id REQUEST_ID
platter eligibility CAST_JOB_ID false
platter eligibility CAST_JOB_ID true
```

Preparation starts or continues the applicable captured workflow without sending.
`prepare-daily` prepares the ready pool. Fresh restart and regeneration have the
terminal-job and identity preconditions below. Eligibility is an explicit
selection change. Optional `--stop-after-seconds SECONDS` applies to preparation,
daily preparation, regeneration, and the URL-selected run defined by
`platter.editions`. Use `platter.packet.prepare` for ordered operating steps.

## Eligibility and source readiness

`jobs.eligible` is the explicit selection policy. Preparation/source readiness
is checked separately. Creating or sending an edition does not derive this
field from history. The ordinary preview operation atomically freezes the
edition and sets selected jobs ineligible. Declining preparation also sets
eligibility false. Posting retrieval failures also set eligibility false,
including HTTP errors, timeouts and unsupported or incomplete posting text.
A failed initial retrieval retains the job without creating a preparation run.
A failed freshness retrieval retains its prepared run as deferred. Changed
postings become stale and ineligible. The eligibility command explicitly
enables a job again; older runs and artifacts remain retained. An enabled
deferred packet must pass freshness before selection.
`run-ad-hoc` explicitly enables its URL-selected job before normal preparation.
Its ordinary one-packet freeze sets that job ineligible again.
Delivery records retain what happened even after eligibility changes.

Preparation reads supported Cast export and Annals work list/show interfaces.
Vita is the fixed Annals library named `vita`. Platter invokes
`~/.local/bin/annals library vita work list --limit 1000 --json` and reads each
returned work with `work show LABEL --json`. It does not call CRM.

Each work becomes one career entry. Its work label is the entry ID; the first
heading is its title, or the work label when no heading exists. Its complete
text becomes the entry body. Platter uses all listed works and rejects an empty
or incomplete list. Works added after that list are available to a later
preparation. Reads start no Annals model work. Read `annals.corpus.explore` for
the supported Annals read contract.
Read `annals.work.retain` and `annals.library.operate` for separately authorized
career-library operations.

Cast exports contain stored posting excerpts. Platter fetches full posting text
through a supported adapter before writing. Sources include Greenhouse, Ashby,
Lever and supported JobPosting JSON-LD. Retrieval fails when pages require
unsupported forms or login, or omit full text. Canonical supported ATS
identities and normalized URLs identify opportunities. Reposts without shared
identifiers can remain separate.

For `run-ad-hoc`, Platter first asks Cast to resolve or collect the exact public
job URL. Cast collection contract 5 permits this explicit selection from a
disabled source or an ATS excluded from ordinary collection. Cast retains only
the requested posting and preserves source enrollment and board-scan state.
Platter then uses that normal job through the same export and preparation path
as scheduled work. A URL
that names only an ATS board, an unsupported page or no unique owned
`JobPosting` fails before packet preparation.

Ashby boards are cached as private `ashby-cache/BOARD.json` files under the
canonical runtime root. Each file contains the complete board response and its
`retrieved_at` download time. Preparation and preview reuse that board for less
than 14 days. A missing or invalid cache, an expired or future-dated download,
or a requested posting absent from the cached board triggers one download. Only a successful
download with a valid jobs array replaces the file. A refresh failure leaves
the old file intact but fails that retrieval; it does not use expired data.

Ashby board downloads have no byte cap. The 30-second HTTP timeout and the
1,000,000-byte limit on each selected posting remain. Other posting responses
retain their 4,000,000-byte cap. Cached posting timestamps report the board's
download time, not the time it was read from disk. The cache is disposable and
is disposable. Remove a board's cache file to force its next
retrieval to download again. This does not change existing captured packets.

## Tailored preparation

Tailored preparations capture `generation=weaver_projects_v1` and the exact Cell,
Wrought and shortening directions. Platter appends the captured project editorial
policy to each direction, so initial project writing and shortening use the
same project policy. Jackson uses the separate resume editorial policy.
Historical captures retain their original directions.
Platter calls the installed Weaver client sequentially before its own draft job.
Each initial direction asks for at most
three short-form resume bullets, with one medium-length sentence per bullet. The first
line selects “bullet points for cell” or “bullet points for wrought”. Weaver
owns its Annals reads and authoring. Platter retains the request IDs before
calling Weaver and stores the returned documents as private immutable artifacts.

Weaver Markdown must contain one flat unordered list with one through three
items. Platter converts text, emphasis and inline code into plain bullet text,
joins wrapped lines, and escapes LaTeX. It does not rewrite claims or supply
source notes. Empty output, clarification questions and unsupported structures
fail preparation. A bullet permits at most 1000 characters and no control
characters. Concision is requested through the direction; length is also subject
to the one-page renderer.

The captured template permits changes only to Jackson bullets and the marked
Cell and Wrought bullet lists. The complete marker and import contract belongs
to `platter.materials`. Missing markers stop new preparation before writing.

Platter checks the project bullets against the template's captured Jackson text.
A content or layout rejection permits one shortening round through Weaver's
revision operation, using the captured shortening direction. Both projects are
revised once, with the same three-bullet limit. A second rejection stops work.
Renderer infrastructure failure stops immediately. All returned documents remain
retained; `project-bullets` records the accepted pair.

The Platter draft job receives the captured posting, career data and fixed
project bullets. It writes the brief and Jackson bullets and checks its work
against the captured editorial policy. It has career-read and `submit_draft`
tools; workspace, local execution and web access are disabled. The immutable
`platter/draft/2` toolset excludes project fields. Platter inserts the retained
project bullets after validation. Final brief, resume content, LaTeX and PDF
commit together before acknowledgement. Layout rejection permits correction of
Jackson within the same draft job. No additional editorial agent is used.

The draft uses `gpt-5.6-sol` at max effort. Brief sections remain Why it works
(at most 45 words), Role (at most 30) and optional Culture (at most 25), with at
most 90 words total. Role and Culture are flat specifics. Unsupported culture
is omitted. Declining retains its assessment without a resume and sets the job
ineligible. Project authoring has already occurred when the draft declines.

Why it works gives one or two direct, evidence-grounded sentences. Role states
stack, responsibilities, and process expectations in one or two short lines.
Culture states supported working norms in one or two short lines. Role and
Culture make no comparison with the user's experience. Displayed briefs omit
caveats, downsides, and hedging; pursuit criteria remain private. Career
references identify the captured entries used by each stage. Culture uses only
the posting or captured material; unsupported Culture is omitted without extra
research or an uncertainty note.

Rendering checks overflow, missing characters, extractable project text, the
Jackson heading and one-page layout. A tailored packet is ready only after an accepted brief,
assembled resume content and a validated retained PDF. A Weaver document alone
is not a ready packet. Weaver certifies neither factual claims nor complete
historical research coverage.

Resume a preparation through the same Platter command. Its saved Weaver IDs and
directions reuse the original assignments. Quota deferral preserves work and
returns the ordinary quota outcome. Terminal failures receive no replacement
job. Saved Weaver prose survives a later runtime failure, which remains an error.
Deadlines and maintenance include only the Weaver jobs recorded for this packet,
along with its Platter jobs. No source-system repair or direct Codex fallback is
part of preparation.

Historical `single_draft_v1` runs retain their combined brief/Jackson/projects
writer, read-only local Annals research, model-authored project descriptions and
source notes, original toolset and complete-project rendering boundary. Captures
without `generation` retain their previous brief/resume or draft/review/revision
workflow. Their captured instructions, accepted artifacts and template references
remain unchanged.

## New and repeated preparation

For an authorized restart after a failed or cancelled preparation, use
`prepare CAST_JOB_ID --fresh`. The latest run must be incomplete, with no
accepted resume, and its model jobs must be terminal or absent. The job must
remain eligible. This operation captures the posting, career entries and
template again. It creates a new run and new model jobs without copying prior
outputs, requests, transcripts or error feedback. Older runs and accepted
artifacts remain retained. Ordinary preparation still resumes retained work.
Fresh preparation uses the same Ashby cache policy.

To create another packet for a previously prepared job, including one already
used in an edition, run `regenerate CAST_JOB_ID --id REQUEST_ID`. It captures
source inputs and current writing instructions again, then uses the ordinary
draft and mechanical validation pipeline. It does not copy prior outputs
or model context, change old packet statuses, enable an ineligible job, freeze
an edition or send email. Availability, compensation and pursuit checks still
apply. Retrieval failure or a declined brief still sets eligibility false.
Successful regeneration leaves the current eligibility policy unchanged.

The request ID contains 1 through 80 ASCII letters, digits, underscores or
hyphens and belongs to the regeneration namespace. It is retained atomically
with the new run's captured inputs. A failure before capture creates no run
or request binding. Reuse for another Cast job is refused. Repeating a captured
request resumes that exact preparation or returns its retained outcome without
recapturing sources. A terminal stage failure remains a failure; another attempt
requires a new request ID. Other model jobs for the opportunity must be terminal
or absent before creating or resuming a regeneration. Maintenance admission
serializes the entire command with other mutations.

The result prints the packet ID and status, plus retained brief and final PDF
artifact IDs when available. Use export to write an artifact, or select that
packet in a new retained-material edition for a separately authorized send.
Older packets, immutable artifacts and frozen delivery records remain intact.
Regeneration uses the existing posting cache policy and has no time guarantee.

## Rendering and deadlines

Rendering uses Tectonic and Python 3 with pypdf. Absolute `PLATTER_TECTONIC` and
`PLATTER_PYTHON` overrides are supported; otherwise resolution checks
`~/.local/bin`, `/usr/local/bin`, `/opt/homebrew/bin` and `/usr/bin`.
Disposable renderer files and redirected caches live beneath the canonical
Platter root and are removed after rendering; abandoned renderer directories
are removed on a later rendering invocation. This is not memory-only rendering.
SQLite journals stay with its database and query temporary storage uses memory.
Installed programs, packages and system fonts remain separate dependencies.
PDF inspection preserves Python's user-package lookup, as in `doctor`, and
disables bytecode writes. Tectonic uses a temporary home. A renderer executable
or execution failure stops preparation and either run command, and requests
cancellation of the exact model job. It is not sent to the model as content
feedback. Repair the dependency before an explicit fresh preparation. Content
and layout rejection still allows revision within the current model job.

There is no candidate or token budget. An optional
`--stop-after-seconds SECONDS` on `prepare`, `prepare-daily`, `regenerate` or `run-ad-hoc`
requests cancellation of the exact live Nucleus job at the deadline and
retains progress. The normal external
source, rendering and execution timeouts still apply. Three ready packets is
an edition ceiling, not a quota. The stored 09:00 America/Chicago setting does
not install or authorize a schedule.

## Daily brief preparation

`prepare-daily`, `run-daily`, and ordinary `preview` honor the optional daily
PDF configuration in `platter.materials`. Each new daily invocation validates
its PDF snapshot before source or model work and before eligibility changes.
There is no fallback from an invalid configured PDF to generation.

With an override, new runs capture `generation=daily_brief_v1`. They retain the
posting, Vita career entries and prompt selection, then use the existing brief
stage and career-read tools. They capture no template or project directions and
invoke no Weaver writer or resume renderer. An accepted pursuit brief makes the
run ready for daily override selection. A declined brief sets the job ineligible.
Posting, compensation, preference and freshness checks still apply. Exact brief
requests and accepted results retain the ordinary interruption and failure rules.

Single-job `prepare`, `prepare --fresh`, `regenerate`, `run-ad-hoc` and retained-
material preview continue to use tailored resumes. They ignore the daily override.
A brief-only run cannot supply a tailored edition. Changing between workflows
starts a new capture when the latest packet cannot serve the selected workflow;
prior model jobs must be terminal or absent. Historical runs and artifacts remain
intact. Incomplete tailored work is not resumed through the daily brief stage.

The edition freezes its independent packet selection and the shared PDF as
defined by `platter.editions`. A ready brief-only run has no generated resume
PDF; it cannot serve a tailored edition.

## Retained content and recovery

Accepted outputs are immutable by run and kind. A repeated submission resolves
to existing identical content; conflicting content is refused. Platter can
recover mailbox work from exact retained requests. Nucleus terminal status
alone does not establish domain success; an accepted artifact survives later
runtime failure. Failed/lost attempts and unresolved delivery can require an
explicit recovery change; do not edit SQLite to force success.

## Prompt selection

New request preparation requires initialized private Bazaar state and a complete
`cell.prompts.platter` selection. The default database is
`~/.local/share/bazaar/bazaar.sqlite3`; an absolute `CELL_BAZAAR_DATABASE` override
is supported. Reads fail without creating state or using embedded fallback text.

The selected string is `{"schema_version":1,"entries":{"PROMPT_ID":VERSION}}`.
Every required component is pinned to a positive version. Missing or invalid
selection stops new request preparation before model admission. Component text
alone does not change the selected set. Platter owns selection meaning and
freezes resolved instructions with each request or captured domain snapshot.
Later text or selection edits do not rewrite saved work.

Keep migration selection version 1 and all referenced text versions for
compatibility. Runtime never imports that seed and deployment supplies no
missing prompt content. New project captures require the separate project
editorial policy and all three captured directions; Jackson retains its own
resume editorial policy. Models, permissions, schemas, tool execution, accepted
content, and recovery remain Platter-owned.

Use `platter.packet.prepare` for the authorized prompt-update procedure and
`bazaar.string.update` for append/recovery semantics.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

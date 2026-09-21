# Prepare private job packets

Platter captures Cast opportunities and Vita career entries. It uses Nucleus to
prepare a concise brief and tailored Jackson content. Weaver supplies project
bullets. Platter freezes editions for authorized delivery through Email. Platter owns retained content, job
eligibility and delivery outcomes. It does not discover jobs, edit source libraries, apply
to employers, contact them or activate a schedule.

## Storage and commands

The canonical runtime database is `packets.sqlite3` under
`~/.local/share/platter`. A sole predecessor `~/.local/share/job-packets`
directory remains the canonical location in place. Two existing roots are
ambiguous and are refused. `--state-dir`, if supplied, must equal the canonical
root; it cannot create an independent live library that bypasses maintenance.

```sh
platter init --resume /absolute/original-resume.tex
platter prepare CAST_JOB_ID
platter prepare CAST_JOB_ID --fresh
platter regenerate CAST_JOB_ID --id REQUEST_ID
platter prepare-daily
platter run-ad-hoc JOB_URL --id OCCURRENCE_ID
platter preview YYYY-MM-DD
platter status
platter eligibility CAST_JOB_ID false
platter eligibility CAST_JOB_ID true
platter export ARTIFACT_ID /absolute/chosen/resume.pdf
```

Initialization imports the supported original LaTeX source into an immutable
artifact. The template must have the expected Jackson National Life bullet
structure. New preparations also need one `\section{Projects}` or
`\section{Side Projects}` ending at the next section or document end. Explicit
`% PLATTER PROJECTS BEGIN` and `% PLATTER PROJECTS END` comments can instead
bound that content. The Jackson and projects regions must not overlap.
Historical preparations require only their original Jackson region.
Its original path is provenance; subsequent work reads its bytes
from SQLite. Configuration, captured inputs, execution correlation, accepted
brief and resume content, generated LaTeX/PDF bytes, editions, receipts and
maintenance holds all live in that database. Artifact IDs are not file paths.
An explicit export writes a private new file at the supplied destination and
refuses to overwrite one. Platter never relies on exported copies.

The core records are jobs, runs, artifacts, editions and ordered edition
attachments. A packet is a run and its artifacts. Content references its
producing run; imported templates have no producing run. Nucleus owns tool
and execution history. Platter retains exact requests and compact execution
progress needed for recovery, without a separate tool-receipt ledger.

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

## Preparation and readiness

Preparation reads supported Cast export and Annals work list/show interfaces.
Vita is the fixed Annals library named `vita`. Platter invokes
`~/.local/bin/annals library vita work list --limit 1000 --json` and reads each
returned work with `work show LABEL --json`. It does not call CRM.

Each work becomes one career entry. Its work label is the entry ID; the first
heading is its title, or the work label when no heading exists. Its complete
text becomes the entry body. Platter uses all listed works and rejects an empty
or incomplete list. Works added after that list are available to a later
preparation. Reads start no Annals model work. The [Vita guide](vita.md)
describes the library and its supported commands.

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
1,000,000-byte limit on the selected posting remain. Other posting responses
retain their 4,000,000-byte cap. Cached posting timestamps report the board's
download time, not the time it was read from disk. The cache is disposable and
is excluded from SQLite backups. Remove a board's cache file to force its next
retrieval to download again. This does not change existing captured packets.

New preparations capture `generation=weaver_projects_v1` and the exact Cell,
Wrought and shortening directions. Platter calls the installed Weaver client
sequentially before its own draft job. Each initial direction asks for at most
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

The template has fixed Cell and Wrought presentation and exactly one marker pair
for each bullet list: `% PLATTER CELL BULLETS BEGIN` / `END` and
`% PLATTER WROUGHT BULLETS BEGIN` / `END`. Each END marker repeats the full
prefix, for example `% PLATTER CELL BULLETS END`. The markers must be inside
Projects and must not overlap. Headers, descriptions, technology lists, links,
dates, project order and all other template bytes remain fixed. Jackson bullets
remain separately editable. Missing markers stop new preparation before writing.

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

Rendering checks overflow, missing characters, extractable project text, the
Jackson heading and one-page layout. Platter is ready only after accepted brief,
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

Import a complete fixed resume template for future runs:

```sh
platter import-template /absolute/private/resume.tex
```

Use this operation for an explicitly requested layout, font or fixed-content
change. It requires initialized state, a valid Jackson bullet region, a separate
Projects region and both fixed project bullet marker pairs. It retains a new
immutable template and selects it atomically. Existing templates, captured runs,
instructions, configuration and editions remain unchanged. The import does not
compile the template, run models or send mail. Check the rendered layout before
delivery. Keep private resume sources outside the repository.

To limit a template update to the Projects section:

```sh
platter import-projects-template /absolute/private/resume.tex
```

The import requires both bullet marker pairs and permits changes only inside the
existing Projects section. It stores a new immutable template artifact and
selects it for future captures. Existing runs keep their previous template.
The old template remains retained. This operation starts no model work and sends
nothing. Private resume bytes must remain outside the repository.

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

## Editions and sending

For one explicitly selected job URL and one authorized email:

```sh
platter run-ad-hoc 'https://jobs.ashbyhq.com/COMPANY/JOB_ID' \
  --id OCCURRENCE_ID
```

The occurrence ID contains 1 through 80 ASCII letters, digits, underscores or
hyphens. It uses the existing ad hoc edition namespace. A new occurrence
captures the configured local date, asks Cast for the normal job record,
prepares or resumes its normal packet, performs the ordinary freshness check,
freezes one ordinary edition and sends it. It creates no separate packet,
edition or workflow type. One mutation admission lock covers the complete
operation.

The command uses the existing packet when normal preparation already has a
ready one. It explicitly enables the selected job, and the successful freeze
sets it ineligible like daily selection. A declined, stale or unavailable
packet creates no edition and sends no email.

An existing occurrence takes the send path before Cast or preparation work. A
frozen occurrence sends its retained bytes, an accepted occurrence returns its
receipt without resending, and an uncertain occurrence remains held. The
original date stays fixed across retries. Reusing the ID for another URL or an
existing multi-packet retained-material edition is refused. Invoking
`run-ad-hoc` is the explicit authority for this one send to Email's fixed
personal recipient.

For a daily edition with applicable send authorization:

```sh
platter run-daily
```

This single command captures the date in Platter's configured time zone after
admission, then prepares the ready pool, performs ordinary preview for that
date, and sends the frozen edition. One mutation admission lock covers all
three operations. The date stays fixed if preparation crosses midnight.
If that date already has an edition, the command uses the existing send path
before any preparation: a frozen edition sends its exact retained bytes, an
accepted edition returns its recorded result without resending, and an
uncertain edition fails without retrying or preparing replacement packets.
No ready packets means no edition or email. A later explicit invocation may
try an empty day again. Missed dates are not backfilled. `prepare-daily` and
`run-daily` skip jobs whose postings cannot be retrieved, mark them ineligible,
and continue to other candidates. A declined opportunity also permits the next
candidate. If the final freshness check excludes the entire ready pool,
`run-daily` prepares other candidates before trying preview again. Other
preparation errors and failed Cast exports stop the run before sending.
Accepted artifacts and frozen delivery records remain retained. Single-job
preparation reports its retrieval error without selecting a replacement job.
Output contains the edition date, delivery status and selected packet count,
or the no-edition result. It does not print the message body or PDF bytes.

An explicit user instruction to enable daily sending supplies standing
authority for each ordinary daily brief and its resume attachments to Email's
fixed personal recipient. Without that authority, use preparation or preview
only. A separately managed Clockwork binding may invoke the exact installed
`run-daily` binary at 18:00 machine-local time. `platter schedule-definition`
prints the selected release's product-owned schema-two definition with
`halt-until-approved`; it does not register or enable a binding. Activation is a separate
authorized operation under `clockwork.schedule.operate`; the installer and
stored 09:00 fields do not enable it. Platter status and doctor report the
schedule as external and do not probe the binding. Email's installed wrapper
loads its existing credential; no credential belongs in a schedule definition.
Starting at 18:00 makes no promise about completion or inbox arrival time.

The first abend creates a durable Clockwork halt and one incident email. Use
`clockwork incident list platter/daily` and `clockwork incident show INCIDENT_ID`
to inspect it. After repair, explicitly approve future scheduling with
`clockwork binding resume platter/daily INCIDENT_ID`. This does not retry a
preparation, reconcile uncertainty, or replace an edition or send key. Binding
changes, deployment and maintenance release preserve the halt.

Normal preview retrieves posting text again, marks unavailable packets deferred
and ineligible, and marks changed packets stale and ineligible. Ashby uses the
shared cache, so changes and closures can remain undetected until its next
download, up to 14 days later.
These readiness decisions remain Platter's expected outcomes; a declined or
stale packet and an empty ready pool are not an abend.
Existing frozen editions return stored contents without
another fetch. An edition stores exact subject and body, a stable idempotency
key, delivery status and receipt. Its ordered attachments reference immutable
PDF artifacts and their filenames. It does not copy PDFs to a directory.

For an already authorized edition:

```sh
platter send YYYY-MM-DD
```

Platter pipes the exact body and base64 attachment bytes through Email's
`--payload-stdin` interface. The selected Email command must advertise that
extension. It retains its fixed personal recipient and credential authority.
Platter durably marks sending before invoking Email, and retains `Accepted ID`
only after recognized acceptance. Acceptance is not Gmail receipt, an
application, or an employer response. An accepted edition is not resent;
interrupted or uncertain attempts stay held. There is no automatic ambiguous
send reconciliation or replacement-job retry.

For another edition made exclusively from retained material:

```sh
platter preview YYYY-MM-DD --ad-hoc RUN_ID --packet PACKET_ID
platter send YYYY-MM-DD --ad-hoc RUN_ID \
  --email-executable /absolute/private/email-wrapper
```

`--ad-hoc` remains a CLI selection operation, not an edition type. These
editions use the same schema, ordinary subject format and sending behavior.
The operation leaves job eligibility untouched and performs no Cast, Annals,
source retrieval, Nucleus or rendering work. Without `--packet`, it selects up
to three retained complete packets in ID order. Repeated `--packet` specifies
one through three distinct packets. `RUN_ID` contains 1 through 80 ASCII
letters, digits, underscores or hyphens. Reuse returns the frozen edition;
explicit date/selection changes are refused. Legacy TEST subjects remain exact.

Optional `--brief-overrides /absolute/paragraphs.json` imports a mapping of
selected packet IDs to valid brief text into the frozen message body. The
original accepted artifacts remain immutable. When an override is supplied
again, recomposed body text must agree with the existing edition. A new ID is
needed for changed content. The override file is not a continuing dependency.
An Email executable override affects only that send and must be absolute.
Every external send still requires its own applicable user authority.

## Recovery and privacy

A schema-six SQLite snapshot contains the entire retained Platter library.
Use the maintained migration/backup operation rather than copying an open
main database without its journal. Schema-one through schema-five state must pass
the explicit [installation migration](install-operate.md); ordinary work refuses it.

Accepted outputs are immutable by run and kind. A repeated submission resolves
to existing identical content; conflicting content is refused. Platter can
recover mailbox work from exact retained requests. Nucleus terminal status
alone does not establish domain success; an accepted artifact survives later
runtime failure. Failed/lost attempts and unresolved delivery can require an
explicit recovery change; do not edit SQLite to force success.

Resume contact details, career history, captured evidence, briefs and supporting
references remain private. Captured material is disclosed through Nucleus to
the model service. Direct project research also exposes selected Krisis text
to Nucleus and the model; Nucleus may retain that tool output. Authorized sends
disclose message and attachments to Email, Resend and Gmail. Source retrieval
discloses HTTP requests to employers.
Nucleus retains its own runtime records and credentials. This storage change
does not relocate other products' state. No completion-time guarantee or final
delivery observer is promised. Installation and migration use the separate
[installation contract](install-operate.md).

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

## Bazaar prompt selection

Prompt preparation requires initialized private Bazaar state and a complete cell.prompts.platter selection. The default database is ~/.local/share/bazaar/bazaar.sqlite3; callers accept an absolute CELL_BAZAAR_DATABASE override. Reads fail without creating state or using embedded fallback text.

Read `cell.prompts.platter` with Bazaar's supported `get` interface. Its content
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
cell.prompts.platter --file /absolute/selection.json`. Use an explicit
`bazaar --database /absolute/private/bazaar.sqlite3` prefix when the caller uses
`CELL_BAZAAR_DATABASE`. To roll back, append the prior selection content. Keep
private text out of logs and retain historical versions.

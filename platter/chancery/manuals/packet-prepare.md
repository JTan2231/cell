# Prepare private job packets

Platter captures Cast opportunities and CRM career entries. It uses Nucleus to
prepare a concise brief and Jackson-only tailored resume, then freezes editions
for authorized delivery through Email. Platter owns retained content, job
eligibility and delivery outcomes. It does not discover jobs, edit CRM, apply
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
structure. Its original path is provenance; subsequent work reads its bytes
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
eligibility false. Changed postings become stale and ineligible. The eligibility
command explicitly changes the field again; enabling a declined or stale job
allows a new preparation run while retaining its older artifacts.
`run-ad-hoc` explicitly enables its URL-selected job before normal preparation.
Its ordinary one-packet freeze sets that job ineligible again.
Delivery records retain what happened even after eligibility changes.

## Preparation and readiness

Preparation reads supported Cast export and CRM profile list/read interfaces.
Cast exports contain stored posting excerpts. Platter fetches full posting text
through a supported adapter before writing. Sources include Greenhouse, Ashby,
Lever and supported JobPosting JSON-LD. Preparation can fail when pages require
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

CRM capture rejects incomplete lists and detected timestamp changes. Separate
list/read calls are not a transactional CRM snapshot, but both stages receive
the same captured library. Models can list/read captured entries and submit
only their stage content. They have no direct database, general filesystem,
shell, web, or messaging access. Source text is untrusted material.

Both Nucleus jobs use `gpt-5.6-sol` with `max` effort. New briefs contain
Why it works and Role sections with optional Culture, at most 90 words total.
Why it works is at most 45 words; Role is at most 30; Culture is at most 25.
Role and Culture are flat specifics rather than comparisons with career
experience. Unsupported culture is omitted without additional research or a
change to pursuit eligibility. The displayed recommendation has no caveats or
hedging; pursuit assessment remains private. Historical paragraph briefs and
version-one requests remain readable with their existing meanings.

Resume submissions contain only plain Jackson bullet contents and their private
career-entry references. Every source byte outside that span remains fixed.
Model text is escaped as LaTeX content. Rendering validates overflow, missing
characters, extractable text, the Jackson heading and one-page layout before
acceptance. Each bullet retains its captured career-entry references.
A run becomes ready only with accepted brief/resume content and retained PDF.

For an authorized restart after a failed or cancelled preparation, use
`prepare CAST_JOB_ID --fresh`. The latest run must be incomplete, with no
accepted resume, and its model jobs must be terminal or absent. The job must
remain eligible. This operation captures the posting, career entries and
template again. It creates a new run and new model jobs without copying prior
outputs, requests, transcripts or error feedback. Older runs and accepted
artifacts remain retained. Ordinary preparation still resumes retained work.
Fresh preparation uses the same Ashby cache policy.

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
`--stop-after-seconds SECONDS` on `prepare`, `prepare-daily` or `run-ad-hoc`
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
try an empty day again. Missed dates are not backfilled. The first preparation
error stops the run before another candidate or an email starts. Accepted
artifacts and frozen delivery records remain retained. A declined opportunity
is an ordinary decision and permits the next candidate. A failed Cast export
stops the run.
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

Normal preview retrieves posting text again, defers unavailable sources and
marks changed packets stale. Ashby uses the shared cache, so changes and
closures can remain undetected until its next download, up to 14 days later.
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
The operation leaves job eligibility untouched and performs no Cast, CRM,
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

A schema-two SQLite snapshot contains the entire retained Platter library.
Use the maintained migration/backup operation rather than copying an open
main database without its journal. Schema-one state must pass the explicit
[installation migration](install-operate.md); ordinary work refuses it.

Accepted outputs are immutable by run and kind. A repeated submission resolves
to existing identical content; conflicting content is refused. Platter can
recover mailbox work from exact retained requests. Nucleus terminal status
alone does not establish domain success; an accepted artifact survives later
runtime failure. Failed/lost attempts and unresolved delivery can require an
explicit recovery change; do not edit SQLite to force success.

Resume contact details, career history, captured evidence, briefs and supporting
references remain private. Captured material is disclosed through Nucleus to
the model service. Authorized sends disclose message and attachments to Email,
Resend and Gmail. Source retrieval discloses HTTP requests to employers.
Nucleus retains its own runtime records and credentials. This storage change
does not relocate other products' state. No completion-time guarantee or final
delivery observer is promised. Installation and migration use the separate
[installation contract](install-operate.md).

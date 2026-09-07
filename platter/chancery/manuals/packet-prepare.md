# Prepare private job packets

Platter captures Cast opportunities and CRM career entries, prepares a concise
brief and a Jackson-only tailored resume through Nucleus, and freezes editions
for explicitly authorized delivery through Email. Platter owns retained content,
job eligibility and delivery outcomes. It does not discover jobs, edit CRM,
apply to employers, contact them, or install recurring delivery.

## Storage and commands

The canonical runtime database is `packets.sqlite3` under
`~/.local/share/platter`. A sole predecessor `~/.local/share/job-packets`
directory remains the canonical location in place. Two existing roots are
ambiguous and are refused. `--state-dir`, if supplied, must equal the canonical
root; it cannot create an independent live library that bypasses maintenance.

```sh
platter init --resume /absolute/original-resume.tex
platter prepare CAST_JOB_ID
platter prepare-daily
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
Delivery records retain what happened even after eligibility changes.

## Preparation and readiness

Preparation reads supported Cast export and CRM profile list/read interfaces.
Cast exports contain evidence rather than complete postings: Platter retrieves
complete supported employer evidence first. Supported sources include
Greenhouse, Ashby, Lever and supported JobPosting JSON-LD. Unsupported forms,
login requirements and missing full text can prevent preparation. A fetch
failure does not establish that an opportunity has closed. Canonical supported
ATS identities and normalized URLs are used; perfect repost deduplication is
not promised.

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
acceptance. These checks and references do not prove every paraphrase faithful.
A run becomes ready only with accepted brief/resume content and retained PDF.

Rendering uses Tectonic and Python 3 with pypdf. Absolute `PLATTER_TECTONIC` and
`PLATTER_PYTHON` overrides are supported; otherwise resolution checks
`~/.local/bin`, `/usr/local/bin`, `/opt/homebrew/bin` and `/usr/bin`.
Disposable renderer files and redirected caches live beneath the canonical
Platter root and are removed after rendering; abandoned renderer directories
are removed on a later rendering invocation. This is not memory-only rendering.
SQLite journals stay with its database and query temporary storage uses memory.
Installed programs, packages and system fonts remain separate dependencies.

There is no candidate or token budget. An optional
`--stop-after-seconds SECONDS` on preparation requests cancellation of the exact
live Nucleus job at the deadline and retains progress. The normal external
source, rendering and execution timeouts still apply. Three ready packets is
an edition ceiling, not a quota. The stored 09:00 America/Chicago setting does
not install or authorize a schedule.

## Editions and sending

Normal preview refreshes posting evidence, defers unavailable sources, and
marks changed packets stale. Existing frozen editions return unchanged without
freshness checks. An edition stores exact subject/body, its stable idempotency
key, delivery status and receipt. Its ordered attachments reference immutable
PDF artifacts, including their filenames. It does not copy PDFs to a directory.

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
Nucleus retains its own separate runtime evidence and credentials; this
storage change does not relocate other products' state. No completion latency,
comprehensive source coverage or final delivery observer is promised.

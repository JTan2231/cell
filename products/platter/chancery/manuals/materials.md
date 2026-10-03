# Retained materials and resume templates

Use this feature to understand Platter's private library, artifact identities,
fixed templates, configuration, and explicit exports. Platter owns captured
bytes and accepted content. Annals owns source career material; Weaver owns its
source reads; Nucleus owns execution history and credentials.

Read `platter.packet.prepare` for initialization, import, configuration, and
export procedures. Read `platter.preparation` for authoring and readiness,
`platter.editions` for frozen selection and delivery, and `platter.maintenance`
for supported storage maintenance.

## Primary interfaces

```sh
platter init --resume /absolute/private/original-resume.tex
platter status
platter export ARTIFACT_ID /absolute/private/chosen/resume.pdf
```

Initialization imports the supported original template. Status reports retained
local work; export creates a new private file and refuses overwrite. Neither
status nor export prepares a packet or sends an email. Template imports and
configuration interfaces are defined below; use `platter.packet.prepare` for
their ordered procedures and authority checks.

## Library and identities

The canonical runtime database is `packets.sqlite3` under
`~/.local/share/platter`. A sole predecessor `~/.local/share/job-packets`
directory remains the canonical location in place. Two existing roots are
ambiguous and are refused. `--state-dir`, if supplied, must equal the canonical
root; it cannot create an independent live library that bypasses maintenance.

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

The core records are jobs, runs, artifacts, editions, selected edition packets
and ordered edition attachments. A packet is a run and its artifacts. Content references its
producing run; imported templates have no producing run. Nucleus owns tool
and execution history. Platter retains exact requests and compact execution
progress needed for recovery, without a separate tool-receipt ledger.

| Record | Owned meaning |
| --- | --- |
| jobs | Canonical opportunity identity, Milieu correlation, employer/title, and explicit eligibility. |
| runs | One preparation's captured inputs, status, timestamps, and compact execution correlation. |
| artifacts | Immutable bytes, producing run when applicable, kind, filename, media type, and integrity hash. |
| editions | Frozen subject/body, date, occurrence identity, send key, delivery state, and acceptance. |
| edition_packets | Ordered selected preparation runs. |
| edition_attachments | Ordered immutable artifact references. |

Settings retain configuration and the selected original-template artifact.
Maintenance holds are owner-keyed rows. A state-directory advisory activity
lock admits live mutations; retained PIDs or leases do not establish liveness.

Current tailored `weaver-cell`, `weaver-wrought`, and optional shortening
artifacts retain the exact Weaver document views. `project-bullets` retains the
accepted plain-text pair. Final `brief`, `resume-content`, `resume-source`, and
`resume-pdf` retain the mechanically accepted outputs. A declined draft retains
its assessment without a resume. Platter keeps no separate Annals snapshot.

Historical `resume-draft`, `resume-draft-source`, `resume-draft-pdf`, and
`resume-review` artifacts preserve their original meaning. A review is exact
UTF-8 Markdown; its organization and judgments are not parsed. The historical
revision adds `proposed_draft` and `editorial_review` to the saved writer request
after review completion. Retained complete-project payloads preserve their
one or two unique Cell/Wrought entries, descriptions, optional dates, one through
eight bullets, and private source notes. Retained text fields permit at most
1000 characters and no controls. Historical rendering supplies
`https://github.com/jtan2231/cell` and `https://wrought.experimental.joeytan.dev`
and omits source notes. These historical payloads do not define the current
brief/Jackson-only draft interface.

Run executions retain exact requests, input fingerprints, compact runtime
state, and attempt correlation. They do not copy Nucleus's tool-call log.
Regeneration also retains its `regeneration_id`, binding the captured Milieu job
and run as defined by `platter.preparation`.

`platter status` reads local preparation and delivery state. It does not prove
current posting availability or inspect the external Clockwork binding.

## Fixed template boundaries

The template has fixed Cell and Wrought presentation and exactly one marker pair
for each bullet list: `% PLATTER CELL BULLETS BEGIN` / `END` and
`% PLATTER WROUGHT BULLETS BEGIN` / `END`. Each END marker repeats the full
prefix, for example `% PLATTER CELL BULLETS END`. The markers must be inside
Projects and must not overlap. Headers, descriptions, technology lists, links,
dates, project order and all other template bytes remain fixed. Jackson bullets
remain separately editable. Missing markers stop new preparation before writing.

## Template selection

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

## Daily PDF configuration

Use one optional, top-level `resume_override` setting to select a private PDF
for future daily editions:

```sh
platter config --resume-override /absolute/private/resume.pdf
platter --json config
platter config --clear-resume-override
```

The path must be absolute and identify a readable, valid, unencrypted PDF with
at least one page. Setting the path validates the file before saving it. Clearing
the setting restores tailored preparation for future daily work. Configuration
changes require ordinary mutation admission. They prepare no packets and send
no email. Keep the PDF outside the repository.

`prepare-daily`, `run-daily` and ordinary `preview` use this setting. A new daily
invocation reads and validates the file before source or model work and before
changing job eligibility. `run-daily` uses that single PDF snapshot through
preparation and freeze. Missing, unreadable or invalid PDFs stop the invocation;
there is no fallback to generation. Validation uses Python with pypdf, but does
not compile LaTeX or impose the generated template's one-page and content rules.

The configured source PDF remains a dependency of future daily work. A frozen
edition owns its retained PDF bytes and no longer reads that file. Read
`platter.editions` for the shared attachment and retry behavior.

## Immutability, recovery, and privacy

The schema-eight SQLite library contains retained inputs, templates, artifacts,
editions, settings, and holds. Schema-one through schema-six state must pass the
explicit [installation migration](install-operate.md); ordinary work refuses it.

Accepted outputs are immutable by run and kind. A repeated submission resolves
to existing identical content; conflicting content is refused. Platter can
recover mailbox work from exact retained requests. Nucleus terminal status
alone does not establish domain success; an accepted artifact survives later
runtime failure. Failed/lost attempts and unresolved delivery can require an
explicit recovery change; do not edit SQLite to force success.

There is no artifact deletion API or automatic domain pruning. A future
retention policy must preserve referenced bytes and delivery uncertainty.

Resume contact details, career history, captured evidence, briefs and supporting
references remain private. Captured material is disclosed through Nucleus to
the model service. Weaver research and historical Platter project research also
expose selected decision text to Nucleus and the model. Nucleus may retain that
output. Authorized sends disclose message and attachments to Email, Resend,
and Gmail. Source retrieval discloses HTTP requests to employers.

Nucleus retains its own runtime records and credentials. No completion-time
guarantee or final delivery observer is promised. Installation and migration use
the separate
[installation contract](install-operate.md).

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

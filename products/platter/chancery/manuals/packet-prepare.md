# Prepare and deliver private job packets

Use this operation to initialize private resume material, prepare or regenerate
packets, freeze an edition, or carry out an authorized send. Platter owns
captured content, eligibility, editions, and delivery records. Nucleus owns
execution; Weaver authors project bullets; Email owns transport acceptance.

Read `chancery resolve platter.packet.prepare` for this procedure and its
required feature contracts. Read one subject with `chancery show ID`:

| Feature | ID |
| --- | --- |
| Retained materials, templates, configuration, and exports | `platter.materials` |
| Source capture, eligibility, authoring, validation, and continuation | `platter.preparation` |
| Edition selection, identity, delivery, and external activation | `platter.editions` |

Use `platter.opportunity.explore` to find retained prepared opportunities. Use
`platter.install.operate` for installation, readiness, migration, and maintained
recovery. Discovery, career-library changes, employer outreach, and application
submission are outside this operation.

## Establish prerequisites and authority

1. Inspect `platter status` and `platter --json doctor`.
2. Confirm the canonical schema-seven library and required sources, runtime,
   and renderer are ready. Ordinary work refuses predecessor schemas.
3. Select preparation, preview, or an applicable authorized send.
4. Keep resume sources, career text, exports, and override files private and
   outside the repository.

Preparation reads supported Cast records and complete posting text, the fixed
Annals `vita` library, and a complete Bazaar `cell.prompts.platter` selection.
Tailored work also needs a supported fixed template, compatible Weaver and
Nucleus, Tectonic, and Python with pypdf. Daily PDF override work validates its
configured PDF before source/model work or eligibility changes; it has no
fallback to generation. Read `platter.preparation` for capture and limits.

Initialization, imports, eligibility changes, configuration, preparation,
preview, and sends use normal mutation admission. A foreign maintenance hold,
unsupported state, incomplete source list, invalid template/PDF, or failed
prerequisite stops the operation. Do not create an independent `--state-dir`
library to bypass maintenance.

Preparation and preview supply no send authority. Each explicit send needs its
applicable user authorization. An instruction to enable daily sending supplies
standing authority for ordinary daily briefs and resume attachments to Email's
fixed personal recipient. A requested `run-ad-hoc` occurrence authorizes that
one selected URL and email. Installation and catalog presence supply neither.

## Initialize or change future material

Import the original supported LaTeX template when state requires initialization:

```sh
platter init --resume /absolute/private/original-resume.tex
```

The template must meet the Jackson and separate Projects boundaries in
`platter.materials`. Its original path is provenance; Platter retains its bytes.
Initialization starts no model work and sends nothing.

For an explicitly requested complete layout or fixed-content change, import a
valid complete template. For a Projects-only change, use the restricted import:

```sh
platter import-template /absolute/private/resume.tex
platter import-projects-template /absolute/private/resume.tex
```

Both imports require initialized state and the fixed project bullet marker
pairs. The Projects-only import permits changes only in the current Projects
section. Each import atomically selects a new immutable template for future
captures and preserves prior templates, runs, and editions. Imports do not
compile, run models, or send. Check the rendered layout before delivery.

Select or clear a private daily PDF, then inspect retained configuration:

```sh
platter config --resume-override /absolute/private/resume.pdf
platter config --clear-resume-override
platter --json config
```

Choose the applicable setter. The path must be absolute and readable and must
identify a valid unencrypted PDF with at least one page. The setter validates
before saving. These commands change future daily work and send nothing.
Frozen editions retain their bytes after the source file or setting changes.
Single-job preparation, regeneration, URL-selected runs, and retained-material
preview continue to use tailored resumes.

## Prepare or continue a packet

Select a Cast job, or prepare the ordinary daily pool without sending:

```sh
platter prepare CAST_JOB_ID
platter prepare-daily
platter status
```

Preparation can consume model allowance and disclose captured career/posting
material through Nucleus. Weaver authors the Cell and Wrought bullets; the
Platter draft authors the brief and Jackson bullets. Tailored success requires
accepted content and a mechanically validated retained one-page PDF. With a
daily override, an accepted pursuit brief can be ready without a generated PDF.
Read `platter.preparation` for the exact current and historical workflows.

Repeat the same command to continue retained requests and Weaver assignments.
Quota deferral preserves progress. An accepted artifact survives later runtime
failure; terminal runtime state alone does not establish packet success.
A terminal failure receives no automatic replacement job or direct Codex
fallback. Inspect the retained result and correct the failed dependency before
an explicitly authorized new attempt.

Single-job retrieval failure reports an error and selects no replacement job.
Daily preparation marks unavailable postings ineligible and tries other
candidates. Declined opportunities and an empty ready pool are expected
outcomes. Other preparation failures and failed Cast exports stop the run.

For an explicitly authorized fresh restart of incomplete work:

```sh
platter prepare CAST_JOB_ID --fresh
```

Require an eligible job, an incomplete latest run with no accepted resume, and
terminal or absent prior model jobs. This captures new inputs and creates new
run and model identities without copying previous outputs or error context.
Older runs and accepted artifacts remain intact.

To create another packet for a previously prepared job:

```sh
platter regenerate CAST_JOB_ID --id REQUEST_ID
```

Use 1 through 80 ASCII letters, digits, underscores, or hyphens. Other model
jobs for the opportunity must be terminal or absent. An exact repeated ID
continues its captured request or returns the retained outcome; reuse for a
different Cast job is refused. A terminal stage failure needs a new ID for
another attempt. Regeneration does not enable the job, freeze an edition, or
send. Retrieval failure or decline can still set eligibility false.

An optional `--stop-after-seconds SECONDS` on `prepare`, `prepare-daily`,
`regenerate`, or `run-ad-hoc` requests cancellation of the exact live Nucleus
job and retains progress. Normal source, renderer, and execution timeouts still
apply. Renderer infrastructure failure stops work and requests exact-job
cancellation; it is not model content feedback.

Change explicit selection eligibility only when intended:

```sh
platter eligibility CAST_JOB_ID false
platter eligibility CAST_JOB_ID true
```

Enabling a deferred packet does not bypass freshness checks. Eligibility changes
leave accepted artifacts and delivery history intact.

## Freeze and inspect an ordinary edition

```sh
platter preview YYYY-MM-DD
```

Ordinary preview retrieves posting text under the source cache policy, excludes
unavailable or changed packets, and atomically freezes up to three ready
packets while making selected jobs ineligible. It sends nothing. Ashby changes
and closures can remain undetected until its next download, up to 14 days later.
A frozen edition keeps its exact body, packet selection, attachment bytes, and
send key. Reopening it does not fetch sources or read a configured PDF again.
No selected packets means no edition.

## Send an authorized occurrence

Send an already authorized frozen edition:

```sh
platter send YYYY-MM-DD
```

Require an Email executable with `--payload-stdin`. Platter sends its exact
retained body and attachment bytes to Email's fixed personal recipient.
Recognized `Accepted ID` means provider submission acceptance; it does not
establish inbox receipt, an application, or an employer response. Verify the
retained edition result. Accepted editions are not resent. Interrupted or
uncertain sends remain held; do not retry with a new send key or edit SQLite.

For authorized daily sending, run:

```sh
platter run-daily
```

The command captures one configured local date under admission, prepares,
freezes, and sends while holding one mutation lock. That date stays fixed
across midnight. An existing occurrence takes its send path before preparation:
frozen sends exact bytes, accepted returns its receipt, uncertain fails held.
An empty pool sends nothing. Missed dates are not backfilled. If freshness
excludes the whole ready pool, the command can prepare other candidates.
Other preparation failures stop before sending.

For one explicitly requested public URL and one email, run:

```sh
platter run-ad-hoc 'https://jobs.ashbyhq.com/COMPANY/JOB_ID' --id OCCURRENCE_ID
```

Use 1 through 80 ASCII letters, digits, underscores, or hyphens. A new occurrence
asks Cast to resolve or collect that exact URL, enables its normal job, prepares
or reuses its tailored packet, checks freshness, freezes one ordinary edition,
and sends. Freeze makes that job ineligible again. Unsupported, ambiguous, or
board-only URLs fail before preparation. A declined, stale, or unavailable
packet creates no edition or email. Exact retries use the original date and
retained send path before Cast or preparation. Reuse for another URL or an
existing multi-packet retained-material edition is refused.

Run commands print the edition date, delivery status, and packet count, or the
no-edition outcome. They do not print message bodies or PDF bytes. Neither a
successful process nor Nucleus completion substitutes for Platter's result.

## Use retained material in another edition

```sh
platter preview YYYY-MM-DD --ad-hoc RUN_ID --packet PACKET_ID
platter send YYYY-MM-DD --ad-hoc RUN_ID \
  --email-executable /absolute/private/email-wrapper
```

Use 1 through 80 ASCII letters, digits, underscores, or hyphens for `RUN_ID`.
Specify one through three distinct complete tailored packets with repeated
`--packet`, or omit it to select up to three retained complete packets in ID
order. This freeze leaves eligibility unchanged and starts no Cast, Annals,
source retrieval, Nucleus, or renderer work. Exact reuse returns the same
edition; changed date or selection is refused.

Optionally supply `--brief-overrides /absolute/private/paragraphs.json` as a
mapping of selected packet IDs to valid brief text. Platter freezes that text
in the body without changing accepted artifacts. Repeated override content
must recompose the same body; changed content requires a new occurrence ID.
The override file is not a continuing dependency. An Email executable override
must be absolute and applies only to that send. Every send requires its own
applicable authorization and retains the same uncertainty rules.

## Export, maintain sources, and operate activation

```sh
platter export ARTIFACT_ID /absolute/private/chosen/resume.pdf
```

Export writes a new private file and refuses overwrite. It does not change
retained bytes; Platter never relies on exported copies. Use
`platter.opportunity.explore` for stable opportunity references and packet facts,
which do not imply an application status or current employer availability.

Read `annals.corpus.explore` for Vita reads. Use `annals.work.retain` or
`annals.library.operate` for separately authorized career-library changes.
Platter's fixed selection is the Annals library named `vita`. Keep source text,
outputs private.

For an authorized prompt update, use `bazaar.string.update` with the initialized
private database used by the caller. Append each component, read its returned
positive version, and then append the complete `cell.prompts.platter` selection.
The selection must include every required component; editing one text does not
change the selection. Use private UTF-8 files for multiline content:

```sh
bazaar update PROMPT_ID --file /absolute/private/prompt.txt
bazaar update cell.prompts.platter --file /absolute/private/selection.json
```

Use an explicit `bazaar --database /absolute/private/bazaar.sqlite3` prefix when
the caller uses `CELL_BAZAAR_DATABASE`. Inspect the committed component and
selection versions. Bazaar does not validate Platter's JSON or prompt meaning.
Keep selection version 1 and all referenced historical texts. A missing or
invalid selection stops new request preparation before model admission; saved
work retains its frozen instructions.

If an append receipt is lost, inspect history before appending again. Repetition
creates another version. To roll back future selection, append the earlier
complete selection content; never delete or rewrite history. Import the reviewed
migration seed before deploying callers, through the supported Bazaar setup
route. Runtime and deployment do not supply missing prompt content. Keep private
text and receipts outside logs and source. Read `platter.preparation` for exact
selection/capture semantics and `bazaar.string.update` for its recovery rules.

Recurring activation requires separate authorization and the installed
`clockwork.schedule.operate` procedure. `platter schedule-definition` only
prints a definition. The ordinary binding is `platter/daily` at 18:00 machine
local time; inspect its actual selected definition. Stored 09:00 fields do not
activate it or promise delivery time. After a failure, inspect the exact
Clockwork incident and Platter result. Repair domain work first, then explicitly
resume that incident for future scheduling. A resume does not retry preparation
or reconcile uncertain delivery. Deployment and binding changes preserve halts.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

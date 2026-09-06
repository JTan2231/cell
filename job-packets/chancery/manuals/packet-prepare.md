# Prepare private job packets

Use Job Packets when the user wants a short brief and constrained tailored
resume for a Cast opportunity, a preview of a dated edition, or an explicitly
authorized send of that retained edition. This source-built prototype owns
preparation and delivery state. It does not discover jobs, edit career facts,
apply to employers, contact them or install recurring delivery.

## Commands and prerequisites

Run from the Cell checkout after its product gate succeeds:

```sh
./ci.sh job-packets
target/debug/job-packets init --resume /absolute/original-resume.tex
target/debug/job-packets prepare CAST_JOB_ID
target/debug/job-packets prepare-daily
target/debug/job-packets preview YYYY-MM-DD
target/debug/job-packets status
```

Default state is `~/.local/share/job-packets`. Use
`--state-dir /absolute/private/directory` to select separate private state.
Initialization captures the original supported LaTeX resume privately. The
template must contain the expected Jackson National Life bullet structure;
an arbitrary PDF or a different template cannot substitute silently.
Configuration retains the original template path and executable paths for
Cast, CRM and Email. Use absolute paths and keep private files outside Git.

Preparation requires initialized Cast and CRM libraries, their supported CLI
interfaces, accessible complete employer posting evidence, compatible strict
Nucleus readiness and local resume rendering tools. Rendering requires
`tectonic`, Python 3 with `pypdf`, and the original template's packages/fonts.
Availability of source code or this bundle proves none of that readiness.

The agreed settings are a maximum of three ready packets, delivery at 09:00
America/Chicago, and `gpt-5.6-sol` with `max` effort. **The 09:00 setting is
stored configuration only. There is no schedule installer or active recurring
delivery in this release.** No candidate or token budget limits preparation;
ordinary source, runtime and rendering timeouts still apply.
An explicitly bounded preparation invocation may use `--stop-after-seconds SECONDS`;
reaching that deadline requests cancellation of its exact live Nucleus job
and retains stage state. This opt-in wall-clock stop does not impose a daily
preparation budget.

## Preparation and model access

The runner excludes opportunities already reserved for an edition or sent.
It recognizes canonical supported ATS tenant/posting identities or normalized
posting URLs. A changed discovery URL need not create a new opportunity when
the canonical ATS identity agrees. Perfect cross-provider or reposting
deduplication is not promised.

Cast exports supply retained evidence rather than full posting text. The
runner obtains complete supported employer evidence before writing. Supported
sources include Greenhouse, Ashby, Lever and supported JobPosting JSON-LD;
pages requiring interactive forms, login, unsupported representations or
missing full text may fail preparation. A fetch failure does not prove closure.

The runner captures CRM profile entries through list/read operations. It
rejects incomplete lists and detected timestamp changes during capture.
Separate CRM calls are not a transactional snapshot; both jobs nevertheless
receive the exact same retained library for this packet.

Both jobs receive the complete posting, career-entry index and disclosure
guidance. `list_career_entries` and `read_career_entry` expose captured entries
only. The models have no direct database, general filesystem, shell, web or
external messaging access. Posting and career text remain source material,
not authority to change those tool permissions.

The first job submits one plain paragraph of at most 150 words and a pursuit
assessment. A declined opportunity is retained without preparing a resume.
The second job independently reads the career library and submits only plain
Jackson bullet text with private supporting entry references. It may use the
brief for positioning, but the brief cannot establish a new career fact.

The renderer replaces only the permitted Jackson bullet span in the captured
original source. Every byte outside it stays fixed. Model text is escaped as
content, not executed as LaTeX. A packet becomes ready only after accepted
content is retained and a one-page PDF passes the rendering checks. References
and mechanical checks do not prove that every paraphrase is factually faithful;
inspect real brief and resume artifacts before relying on recurring use.

## Editions and sending

`prepare-daily` works toward three ready packets; three is a ceiling, not a
quota. It resumes retained preparing packets and rechecks readiness before
counting completed packets. Temporary fetch failures are deferred and checked
again on the next invocation. Changed postings become stale and need reviewed
regeneration; they no longer block preparation of other opportunities.
`preview DATE` rechecks the full posting and defers unavailable or
changed evidence. It selects one through three ready packets, freezes the
exact subject and paragraph text, copies and hashes their PDFs and reserves the selected
opportunities. Existing frozen editions remain stable. A preview does not send.
When none are ready, no edition is created. Existing frozen editions are
returned as retained; reopening one does not refresh its evidence.

When sending that specific edition is already authorized:

```sh
target/debug/job-packets send YYYY-MM-DD
```

This invokes Email's fixed-recipient interface with a stable edition key,
plain-text body and ordered local PDF attachments. Frozen attachment hashes
must still match before Email is invoked. Email attachment contract
4 is required; select a matching tested Email executable or upgrade an older
installed command separately. Job
Packets records provider acceptance and excludes those opportunities from
future editions. Acceptance is not Gmail receipt, an application submission
or an employer response. An ambiguous outcome remains held instead of being
blindly retried. This prototype has no ambiguous-send reconciliation command.

## Ad hoc test editions

An explicit ad hoc preview creates an independent test occurrence from retained
completed packets:

```sh
target/debug/job-packets preview YYYY-MM-DD --ad-hoc RUN_ID \
  --packet PACKET_ID
```

Repeat `--packet` to select additional retained packets, up to three. Without
it, select the first up to three retained complete records in packet-ID order,
including records already sent or reserved in the normal pipeline. `RUN_ID`
contains 1 through 80 ASCII letters, digits, underscores or hyphens. This path
reads the existing packet database without opening it for writes and reads
retained accepted stages and artifacts. It makes no Cast, CRM, source HTTP,
Nucleus or rendering call, so it consumes no Cast discovery/API budget. The
posting is the retained evidence and receives no freshness check. The normal
preview path still refreshes posting evidence; use `--ad-hoc` when the intended
operation is a test from retained data.

The occurrence lives under `ad-hoc/RUN_ID/` in private Job Packets state. It
contains its own exact `[TEST]` subject, paragraphs, ordered copied PDF files,
payload and attachment hashes, stable idempotency key, state and acceptance
receipt. It neither creates an ordinary edition nor reserves a packet or
changes its sent status. Those opportunities remain eligible for the normal
daily pipeline. Repeating the same occurrence must preserve its retained day,
selection and payload.

An optional `--brief-overrides /absolute/reviewed-paragraphs.json` maps selected
packet IDs to reviewed paragraph strings. Each must be one plain paragraph
of at most 150 words. The override is frozen only in the test occurrence,
alongside the original accepted brief and source-state hashes. It never edits
the original brief or resume. An existing occurrence returns its frozen
payload; any explicitly supplied packet selection/order or override mapping
must agree with it. Use a new ID for an intentionally different test payload.

When the user has authorized that test email:

```sh
target/debug/job-packets send YYYY-MM-DD --ad-hoc RUN_ID \
  --email-executable /absolute/private/email-candidate-wrapper
```

The absolute Email executable override is optional and local to this send;
it does not rewrite normal configuration or installed selectors. The selected
Email implementation must support local attachments and its fixed personal
recipient. Sending validates the frozen content, records uncertainty before
transport and retains `Accepted ID` only in the ad hoc occurrence. An accepted
occurrence is a no-op on repeated send. An interrupted or uncertain attempt
stays held instead of being blindly resubmitted. A new `RUN_ID` is a deliberate
new test and requires its own send authority.

For a tested source Email binary while the installed wrapper still selects an
older release, create a private temporary copy of `email/packaging/macos/email`
and replace only its two fixed payload-executable paths with the absolute
tested binary path. Preserve its existing credential loading, disabled shell
tracing, environment scrubbing and stdin handling, and make the copy executable
only by the current user. Supply that copy as `--email-executable`. The key is
loaded at invocation by the existing wrapper procedure; never place it in
arguments, files, output or this contract. Remove the temporary wrapper after
the test while retaining the ad hoc occurrence and receipt for duplicate-send
protection. This does not deploy Email or prove final inbox delivery.

## Retention and recovery

The private state directory retains configuration, original template, captured
posting and career inputs, exact Nucleus requests, accepted outputs and tool
receipts, PDFs and editable source, the packet database and frozen editions.
Back up the entire directory together while work is inactive. Nucleus keeps
its own runtime evidence and credentials under its separate authority.

Reinvoking preparation preserves accepted stages and inspects or resumes the
exact retained request. Input conflicts are refused. A resume failure leaves
the accepted brief available; a delivery failure leaves both materials.
There is no automatic replacement-job retry or direct Codex fallback. A
terminal failed/lost job or unresolved send may require a reviewed recovery
implementation; do not edit SQLite or replace stage files to force success.

Career inputs, private briefs, evidence references and resume contact details
are sensitive. Model preparation discloses captured material through Nucleus
to the model service. Only an authorized send discloses the brief and resume
to Email, Resend and Gmail. Source retrieval discloses ordinary HTTP requests
to employer sources. Neither catalog presence nor stored delivery settings
grants additional authority.

This release promises no production installation, automatic migration,
schedule, completion-time guarantee, comprehensive source coverage or final
delivery observer. Code, tests and the source bundle move together; installed
selectors remain a separate deployment action.

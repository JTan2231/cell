# Editions and delivery

Use this feature to understand packet selection, frozen message and attachment
identity, authorized delivery, and separately managed daily activation.
Platter owns editions and delivery records. Email and Resend own submission
acceptance; Gmail owns final receipt. Platter never applies to an employer or
contacts one.

Read `platter.preparation` for eligibility and posting freshness, and
`platter.materials` for artifact and daily PDF configuration. Use
`platter.packet.prepare` for prepare, preview, and send procedures.

## Selected packets and frozen attachments

An edition stores exact subject and body, a stable idempotency key, delivery
status, and receipt. Its ordered attachments reference immutable PDF artifacts
and their filenames. Selected packet IDs are independent of attachment IDs.
It does not copy PDFs to a directory. An ordinary freeze atomically sets every
selected job ineligible; retained-material selection leaves eligibility intact.
Delivery history never derives eligibility.

Each ordinary edition selects up to three ready packets. A URL-selected
occurrence selects exactly one. Three is a ceiling, not a quota. Existing
frozen editions return retained contents without another posting fetch.

## Daily shared PDF

A daily edition selects up to three jobs and freezes one shared `resume-override`
PDF artifact with its original filename. Its body identifies the shared resume.
The artifact has no producing run. Selected packet IDs are retained separately
from attachment IDs, so the freeze sets every selected job ineligible. No selected
jobs means no edition or email. A prepared tailored packet can supply its brief
without adding its generated PDF to an override edition.

Already frozen editions retain their exact packet selection, attachments, body
and send key. Opening or sending them does not read the configured file. Accepted
editions are not resent and uncertain sends remain held. Replacing the file or
clearing the setting affects only future editions. SQLite retains frozen PDF
bytes; future daily work still depends on the configured source file.

Single-job preparation, regeneration, URL-selected runs, and retained-material
preview ignore the daily override and require tailored material. A brief-only
run cannot serve a tailored edition.

## URL-selected occurrence

For one explicitly selected job URL and one authorized email:

```sh
platter run-ad-hoc 'https://jobs.ashbyhq.com/COMPANY/JOB_ID' \
  --id OCCURRENCE_ID
```

The occurrence ID contains 1 through 80 ASCII letters, digits, underscores or
hyphens. It uses the existing ad hoc edition namespace. A new occurrence
captures the configured local date, selects one retained job from Milieu's export
by canonical supported ATS identity or normalized public URL, prepares or
resumes its normal packet, performs the ordinary freshness check,
freezes one ordinary edition and sends it. It creates no separate packet,
edition or workflow type. One mutation admission lock covers the complete
operation.

An unknown URL, an ambiguous match, or a supported ATS board URL without a
posting identity fails before preparation. This selection performs no Milieu
collection.

The command uses the existing packet when normal preparation already has a
ready one. It explicitly enables the selected job, and the successful freeze
sets it ineligible like daily selection. A declined, stale or unavailable
packet creates no edition and sends no email.

An existing occurrence takes the send path before Milieu or preparation work. A
frozen occurrence sends its retained bytes, an accepted occurrence returns its
receipt without resending, and an uncertain occurrence remains held. The
original date stays fixed across retries. Reusing the ID for another URL or an
existing multi-packet retained-material edition is refused. Invoking
`run-ad-hoc` is the explicit authority for this one send to Email's fixed
personal recipient.

## Daily occurrence and activation

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
preparation errors and failed Milieu exports stop the run before sending.
Accepted artifacts and frozen delivery records remain retained. Single-job
preparation reports its retrieval error without selecting a replacement job.
Output contains the edition date, delivery status and selected packet count,
or the no-edition result. It does not print the message body or PDF bytes.

An explicit user instruction to enable daily sending supplies standing
authority for each ordinary daily brief and its resume attachments to Email's
fixed personal recipient. Without that authority, use preparation or preview
only. A separately managed Clockwork binding may invoke the exact installed
`run-daily` binary at 18:00 machine-local time. `platter schedule-definition`
prints the selected release's product-owned schema-three definition with
`halt-until-approved`; it does not register or enable a binding. Activation is a separate
authorized operation under `clockwork.schedule.operate`; the installer and
stored 09:00 fields do not enable it. Platter status and doctor report the
schedule as external and do not probe the binding. Email's installed wrapper
loads its existing credential; no credential belongs in a schedule definition.
Starting at 18:00 makes no promise about completion or inbox arrival time.

The first abend ends the current run. Clockwork permits later scheduled
activations before the shared service-health threshold. By default, five
consecutive failed read-only checks, at least 60 seconds apart, halt the binding
and make its alert eligible together. Healthy or inactive checks clear a pending
episode. Accepted editions and uncertain sends retain their existing protections.
Use
`clockwork incident list platter/daily` and `clockwork incident show INCIDENT_ID`
to inspect it. After repair, explicitly approve future scheduling with
`clockwork binding resume platter/daily INCIDENT_ID`. This does not retry a
preparation, reconcile uncertainty, or replace an edition or send key. Binding
changes, deployment and maintenance release preserve the halt.

## Freshness and explicit sending

Normal preview retrieves posting text again, marks unavailable packets deferred
and ineligible, and marks changed packets stale and ineligible. Ashby uses the
shared cache, so changes and closures can remain undetected until its next
download, up to 14 days later.
These readiness decisions remain Platter's expected outcomes; a declined or
stale packet and an empty ready pool are not an abend.

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
The operation leaves job eligibility untouched and performs no Milieu, Annals,
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

Accepted content remains immutable. An accepted edition is not resent; an
interrupted or uncertain send remains held. There is no automatic ambiguous
send reconciliation, replacement send, or employer delivery observer.
Do not edit SQLite to force success.

Authorized sends disclose the exact message and attachments through Email,
Resend, and Gmail. Posting freshness retrieval discloses HTTP requests to
employers. Keep resume contact details, job interests, career history, and
frozen editions private. SQLite retains message and attachment bytes. Nucleus
records and credentials remain separate.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

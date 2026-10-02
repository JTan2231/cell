# Retained sources and delivery activity

Use this feature to preserve one unchanged source, retrieve retained works, or
interpret source-delivery activity. Retention stores nonblank UTF-8 bytes or
recognizes bytes already present. It invokes no model, Nucleus, or network and
changes no corpus state. Use `annals.work.integrate` for interpretation.
`annals.libraries` owns library selection, admission kind, read prerequisites,
common output, and Rust client access.

## Retain and read a work

The supported interfaces are:

```text
annals work add UTF8_INPUT [--name LABEL]
annals work add - --name LABEL
annals work list [--limit N]
annals work show LABEL
annals library NAME work add UTF8_INPUT [--name LABEL]
```

A file may omit `--name`; its UTF-8 filename stem becomes the proposed work
label. Standard input requires an explicit label. Work labels are nonempty
and normalized-unique. The selected database must have the immutable `general`
kind. A producer-accepted decisions library rejects this inlet even when
selected directly. Selecting another name or path cannot bypass its role.

Exact retained bytes are content-addressed by SHA-256. Supplying the same bytes
again, even with another proposed label, selects the original work and its
canonical label. A label already attached to different bytes is a conflict.
Annals neither overwrites work nor updates or deletes retained bytes.

Successful `work add` records one manual source delivery. New bytes become
one immutable work; duplicate bytes recognize the existing work. Work retention
and that delivery's completion are atomic. The durable receipt identifies the
work and delivery result. Retention creates no examination, reconciliation,
commit, or corpus revision.

Source material can contain wants, preferences, career accounts, notes, or
decision accounts supplied as ordinary documents to an appropriate general
library. Annals preserves their wording. Retention creates no deadlines,
priorities, workflow transitions, summaries, or inferred application fields.
The dedicated decisions inlet remains restricted to validated Krisis documents
through `annals.decision-account.exchange`.

`work list` defaults to a positive limit of 20. Its schema-two output contains
`items` and `has_more`; increase `--limit` to select more. Human output reports
labels and sizes. JSON also reports SHA-256 and `first_retained_at`.
`work show` returns complete unchanged text, Markdown heading paths, and
`first_retained_at`. Heading paths describe the source document and are not
concept paths. Human output labels the timestamp `First retained`.

`first_retained_at` records when those exact bytes first entered the library.
It remains unchanged across duplicate deliveries. It does not describe source
creation, authorship, publication, or file modification.

## Source deliveries

A delivery is one occasion when source material is supplied. Several deliveries
can select the same work. `work add` and the input form of `integrate` create
manual deliveries. `integrate --work LABEL` selects an existing work and creates
no new delivery. Inbox registration and producer acceptance precede a delivery;
dispatch starts its delivery record. `annals.inbox` owns queue and retry policy.

Delivery lifecycle status is `processing`, `completed`, or `failed`. Retention
disposition is independently `new` or `duplicate`. A successful completed
delivery has one result:

| Result | Meaning |
| --- | --- |
| `retained` | `work add` or a fresh duplicate inbox delivery completed at retention. |
| `pending` | Integration recorded a reconciliation awaiting application. |
| `applied` | Integration created the reported corpus revision. |
| `recorded` | Integration completed without a corpus change. |

A processing delivery has no terminal result. A failed delivery has no result
and has a structured error. It can still identify a work and retention
disposition if failure followed retention. Only `applied` names a corpus
revision. An input integration's applied result and its corpus revision commit
atomically. Completed domain results survive later runtime failure.

Source-bearing manual commands run serially per library. After interruption,
the next such command finalizes an abandoned delivery with
`manual_ingestion_interrupted`. An inbox processing error fails its delivery
on the first attempt. An operator-skipped inbox job appears as a failed
delivery with `inbox_job_skipped`.

Each retry child is a new inbox delivery. Its original remains failed at its
original completion time. Activity reports each delivery independently; use
`inbox retry status` for the original-to-child and event relationships.
Ordinary duplicate retention does not imply another examination or corpus change.

## Read recent delivery activity

The supported activity interface is:

```text
annals lately [--since TIME] [--until TIME]
              [--by created|modified|first-seen|ingested|completed]
              [--status processing|completed|failed]
              [--channel manual|inbox]
```

`lately` reads delivery metadata. It never searches or emits source content,
headings, quotations, topics, or dates mentioned within a work. It is a local
read and invokes no model or network service.

The window is UTC and half-open: `since` is inclusive and `until` is exclusive.
`--until` defaults to the report's start instant; `--since` defaults to `7d`.
A relative `--since` is subtracted from the resolved end, so an explicit end
and relative start produce a reproducible window. Relative durations are a
positive integer followed by `s`, `m`, `h`, `d`, or `w`.

Absolute input is an RFC 3339 timestamp or `YYYY-MM-DD` UTC date. A date selects
midnight at its start. RFC 3339 offsets are accepted and output is normalized
to UTC. The start must precede the end. The selected `--by` basis controls
both inclusion and newest-first ordering:

| Basis | Meaning |
| --- | --- |
| `created` | Filesystem creation time captured at arrival, when supplied by the operating system. |
| `modified` | Filesystem modification time captured at arrival. |
| `first-seen` | When Annals first observed the delivery. |
| `ingested` | When Annals retained new bytes or recognized existing bytes; the default. |
| `completed` | When processing reached completed or failed. |

Filesystem times are captured metadata, not authorship, publication, events
described in the document, or continued observation of its original path.
Standard input has no filesystem times. A pre-retention failure has no ingestion
time. An active delivery has no completion time.

Status and channel filters apply before the window. A delivery without the
selected timestamp cannot enter the window. `missing_time_count` counts all
such deliveries matching those filters. Choose `first-seen` or `completed`
to inspect an early failure omitted by the default ingestion basis.

Human output reports the exact resolved window, basis, filters, lifecycle and
retention counts, and each selected timestamp, channel, status, source name,
result, applied revision, and retention disposition. It states omitted timestamp
counts. Empty windows report `No source activity`.

JSON returns `since`, `until`, `time_basis`, optional `status` and `channel`,
delivery/lifecycle/retention/missing-time counts, and `deliveries`. Each delivery
has `source_name`, `channel`, `status`, `first_seen_at`, and optional `retention`,
`result`, work label, source byte size and SHA-256, `source_created_at`,
`source_modified_at`, `ingested_at`, `completed_at`, `applied_revision`, and
structured `error`. Errors are reporting-safe lifecycle summaries, not raw
runner diagnostics. The report exposes no storage-row ID or dedicated
source-path field.

## Failure, privacy, and limits

Blank or invalid UTF-8 input, a missing stdin label, label conflicts, incompatible
libraries, and wrong admission kinds fail explicitly. Inspect the selected
library and source. Do not replace retained bytes or use the decisions inlet
as an ordinary source path. Read commands never repair missing state.

Sources, headings, work labels, delivery metadata, and outputs can be private.
All retained source text stays in the selected local library. Read access does
not authorize mutation or external disclosure. CLI usage recording follows
`annals.libraries` and does not contain arguments, output, or outcomes.

Retention accepts one nonblank UTF-8 source per invocation. The contract promises
no maximum source size, storage capacity, throughput, or wall-clock latency.
Feature contract version 3, provider release, library schema, and output schema
are independent. No cross-release client compatibility window or deprecation
period is promised. Retention and source reads rely on local Annals state only.

# Cast records and read handoff

Cast owns retained company and job identities, revisions, source metadata,
collection evidence, and read projections. Local callers and downstream
products can inspect that evidence or export one consistent snapshot. Reads
require supported initialized state and start no collection, agent, remote
request, or downstream workflow.

## Read interfaces

```sh
cast companies list
cast jobs list
cast sources list
cast company show COMPANY_ID
cast job show JOB_ID
cast unresolved list
cast search "infrastructure"
cast status --json
cast export --json
cast export --output /absolute/private/snapshot.json
```

Select independent state with the global `--state-dir PATH` option. Output is
readable text by default. Lists and search print compact rows and report when
more results exist. Show prints full retained fields, including posting text.
Status labels retained counts, local budget units, and collection outcomes.

Use the global `--json` flag before or after the command for the supported
machine interface. Its existing success schemas remain unchanged. `snapshot`
is an alias for `export`. With `--output`, the snapshot file remains JSON in
both modes; the flag selects the stdout receipt format. A stdout export prints
a readable snapshot by default or the complete JSON snapshot with `--json`.
Read `cast.state` for state selection and initialization.

The provider-owned Rust read surface is `cast::store::Store::open(&Path)` and
`Store::snapshot()`, returning `cast::models::Snapshot`. Use Cast's exported
types. Direct SQLite reads and writes are not supported integration surfaces.

## Retained record meanings

A company is a stored employer candidate keyed by a discovered domain or an
ATS provider/tenant identity. New means first stored by Cast. Website-domain
and ATS-tenant records can describe the same real employer; a matching name
alone does not merge them.

A job is one observed posting with a source identity, extracted fields,
evidence, observation timestamps, and recorded availability. It is not a
recommendation, application, or hiring outcome. Relevance reasons describe
discovery matches. Cast retains incoming jobs without a title substring
requirement; consumers choose jobs by title, seniority, location, or other
preferences.

A source is a careers collection endpoint associated with a company record.
An observation is source-attributed information retained at a known time.
Coverage describes the pages or items processed by a query or source,
including limits and partial, failed, unsupported, or deferred work. Freshness
is age of successful evidence, separate from the latest attempt.

Company, job, and source IDs are opaque stable Cast identities. Source-native
IDs are scoped to their source namespace. Record revisions track retained
changes. An ownership correction can change a job's `company_id` and revision
while preserving job and source IDs. Read `cast.state` for the repair contract.

Records include domain fields, careers URLs, source locators, and extracted
job fields. Posting descriptions are excerpts of at most 1,200 characters,
with a fingerprint for comparing descriptions. A source locator supports a
separate fetch when a consumer needs posting text; reading the locator causes
no fetch. Whole provider responses and HTML documents are transient.

## Availability and freshness

`first_seen_at` and `last_seen_at` are Cast observation times. Source-supplied
publication and update dates use separate fields. Each job includes its
recorded availability and source collection timestamps. Collection records
separate the latest ordinary source attempt from its last successful
observation.

| Availability | Meaning |
| --- | --- |
| `unknown` | Third-party discovery has not established employer-source availability. |
| `listed` | An employer/ATS source observed the posting. |
| `unlisted` | The source explicitly supplied that flag. |
| `missing` | The posting was absent from a completed employer-source scan. |
| `presumed_closed` | At least two complete scans found it missing, and at least 24 hours passed since the first missing observation. |

Failed or partial scans do not add missing observations. Rapid repeated scans
keep the same 24-hour requirement. A later employer-source observation
restores `listed`.

Attribution identifies the observation source. The adapter assigns
`employer_ats` or `employer_jsonld_owned` under the source ownership rules in
`cast.discovery.collect`. Ownership reconciliation sets older
`employer_jsonld` and affected shared-host rows to `unknown`.

Exact-job collection updates only the selected job and its run record while
preserving board-scan state. Its run note identifies scope `job`, selected
source and URL, and outcome. Source health continues to describe ordinary
board collection. Use the selected job and run note for targeted retrieval.

## Output selection

With `--json`, list and search return schema-two pages with
`snapshot_revision`, compact `items`, and `has_more`. The default limit is 20 items. `--limit` accepts a
positive integer. An empty page has no stored record matching the selected
query and limits; it is not evidence that no external jobs exist.

Job rows contain stable ID and revision, company, title, location, remote
eligibility, recorded availability, and `last_seen_at`. Search matches
substrings in company names, domains, job titles, and descriptions, ignoring
case. Its rows add `matched_field` and a marked excerpt of at most 240 Unicode
characters.

`unresolved` shows companies without domains and sources whose status is not
a successful collection/resolution outcome. JSON status schema 2 returns counts,
budgets, usage, the last run, and collection summaries for sources and queries.
Counts describe retained records and selected collection work. They do not
count all postings available from external sources. Show and export return
full records, source metadata, and collection outcomes.

## Consistent export

The schema-one snapshot contains `schema_version`, `snapshot_revision`,
`captured_at`, `companies`, `jobs`, `source_health`, and query `coverage`.
A snapshot is one consistent current discovery view with identities,
revisions, evidence, and coverage. Export uses one database transaction.
`captured_at` describes snapshot capture; record observation times describe
the retained evidence. Separate list/show calls may observe different
committed states.

`--output` writes a private temporary file, syncs it, atomically replaces the
destination, and syncs its directory. It rejects the selected Cast database,
SQLite sidecars, and mutation lock as destinations, including aliases to
those paths. Stdout reads do not create an export file.

Consumers use stable IDs and record revisions to compare snapshots. They own
consumed state, selected or dismissed jobs, packets, applications, CRM cases,
and sent-message history. Cast has no durable change-feed acknowledgement or
consumer-retention protocol.

## Failure, access, and compatibility

Operational errors exit 1. They use a text diagnostic on stderr by default.
With `--json`, stderr contains one schema-one object with `ok:false` and
`error.detail`. Invalid command syntax uses Clap's text diagnostic on stderr
and exit 2 in both modes. The `status-snapshot --json` operational protocol
remains separate from ordinary command output.

Reads return stored evidence without refreshing it. Use the collection
interfaces when source retrieval is required and the supported ownership
repair when older associations need correction. Do not infer complete source
coverage from an absent error or repair database rows by hand.

CLI output, exports, and diagnostic captures remain private caller-owned
data. They can contain search interests, source locators, and posting text.
Reads send no information to remote systems and grant no authority to contact
an employer or start a downstream workflow.

Read contract 3, list/search/status schema 2, export schema 1, database schema,
and Cast package version are distinct. No maximum database size, export size,
read-latency service level, or future deprecation window is promised. These
reads rely only on local retained Cast state; external source readiness is
not a prerequisite for reading stored results.

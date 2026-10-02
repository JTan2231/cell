# Cast records and read handoff

Cast owns company and job identities, current revisions, source metadata, and
read projections. Local callers and downstream products can inspect retained
fields or export one consistent snapshot. Reads require supported initialized
state and start no collection, agent, remote request, or downstream workflow.

Cast performs no collection. `run`, `job refresh`, and `job collect` are
removed. Existing records, read interfaces, and output schemas remain in use.
Missing state uses the accepted current model in `cast.state`; schema-one
discovery state remains readable without migration. Cast projects both formats
into the existing public read types and schema-one snapshot.

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

In current state, a company is an accepted company identity. A job refers to
its hiring employer through `employer_id`. A source can independently refer to
its operator through optional `operator_id`. Source operator and job employer
can be different companies.

In legacy state, a company is a stored employer candidate keyed by a
discovered domain or an ATS provider/tenant identity. New means first stored
by Cast. Website-domain and ATS-tenant records can describe the same real
employer; a matching name alone does not merge them.

In current state, a job is one accepted current record, with workplace links
and one or more source appearances. Duplicate matching and conflicting source
values are resolved before writing it. The public `company_id` is its
`employer_id`. In legacy state, a job is one observed posting with a source
identity, extracted fields, evidence, observation timestamps, and recorded
availability. It is not a recommendation, application, or hiring outcome.
Relevance reasons describe discovery matches. Reads include all retained jobs
without a title substring requirement; consumers choose jobs by title,
seniority, location, or other preferences.

In current state, a source is a named board or feed with a URL and optional
operator. Its exported `company_id` retains the separate legacy association
used by source controls; it does not establish a job employer or source
operator. In legacy state, a source is a retained careers endpoint associated
with a company record. An observation is legacy source-attributed information
retained at a known time. Coverage describes the pages or items processed by a
query or source, including limits and partial, failed, unsupported, or
deferred work. Freshness is age of successful evidence, separate from the
latest attempt.

Company, job, and source IDs are opaque stable Cast identities. Source-native
IDs are scoped to their source namespace. Record revisions track retained
changes. Current state computes revisions from exported material fields,
excluding `last_seen_at`; it retains only each entity's current revision,
without a revision-history table. Secondary appearance changes and changes
between hybrid and on-site can preserve the export revision while advancing
the snapshot revision. An ownership correction can change a job's
`company_id` and revision while preserving job and source IDs. Read
`cast.state` for the repair contract.

Records include domain fields, careers URLs, source locators, and extracted
job fields. Legacy posting descriptions are excerpts of at most 1,200
characters, with a fingerprint for comparing descriptions. Current accepted
descriptions are stored as supplied. A source locator supports a separate
fetch when a consumer needs posting text; reading the locator causes no fetch.
Cast retains extracted or accepted fields instead of whole provider responses
and HTML documents.

## Availability and freshness

`first_seen_at` and `last_seen_at` in legacy state are retained Cast
observation times. Current state preserves supplied compatibility timestamps
and evidence without creating observation history. It normalizes company/job
first- and last-seen timestamps to UTC with nine fractional-second digits for
chronological string comparison. Writing or exporting an accepted job does not establish that its source was observed at the write or
capture time. Source-supplied publication and update dates use separate
fields. Each job includes its recorded availability and source collection
timestamps. Historical collection records separate the latest ordinary source
attempt from its last successful observation. No command refreshes those
observations or establishes current external availability.

| Availability | Meaning |
| --- | --- |
| `unknown` | Third-party discovery has not established employer-source availability. |
| `listed` | An employer/ATS source observed the posting. |
| `unlisted` | The source explicitly supplied that flag. |
| `missing` | The posting was absent from a completed employer-source scan. |
| `presumed_closed` | At least two complete scans found it missing, and at least 24 hours passed since the first missing observation. |

These meanings describe retained legacy collection evidence. Current state
exports the accepted `status` as `availability`; an accepted value does not
prove a fresh source observation. Failed or partial scans did not add missing
observations. Cast no longer performs scans or updates availability from
external sources.

Attribution identifies the observation source. `employer_ats` and
`employer_jsonld_owned` identify retained employer-source observations.
Ownership reconciliation sets older `employer_jsonld` and affected shared-host
rows to `unknown`.

In schema-one state, historical exact-job collection updated only the selected
job and its run record while preserving board-scan state. Its retained run
note identifies scope `job`, selected source and URL, and outcome. Source
health describes historical ordinary board collection.

## Output selection

With `--json`, list and search return schema-two pages with
`snapshot_revision`, compact `items`, and `has_more`. The default limit is 20
items. `--limit` accepts a positive integer. An empty page has no stored
record matching the selected query and limits; it is not evidence that no
external jobs exist.

Job rows contain stable ID and revision, company, title, location, remote
eligibility, recorded availability, and `last_seen_at`. Search matches
substrings in company names, domains, job titles, and descriptions, ignoring
case. Its rows add `matched_field` and a marked excerpt of at most 240 Unicode
characters.

`unresolved` shows companies without domains and sources whose status is not a
successful collection/resolution outcome. JSON status schema 2 returns counts,
budgets, usage, the last run, and collection summaries for sources and
queries. Counts describe retained records and selected collection work. They
do not count all postings available from external sources. Schema-two state
has empty query coverage, zero request usage, and no last run. These empty
fields do not mean completed collection. Show and export return full records
and source metadata; legacy state also retains collection outcomes.

## Consistent export

The schema-one snapshot contains `schema_version`, `snapshot_revision`,
`captured_at`, `companies`, `jobs`, `source_health`, and query `coverage`. A
snapshot is one consistent current read view with identities, revisions,
compatibility fields, and legacy evidence and coverage when present. Current
state exports one primary source appearance per job through the existing
`source_id`, `source_key`, and `url` fields. Additional appearances remain in
the current store; the existing snapshot does not expose all source links. The
caller supplies the primary appearance when accepting a new job, and that
source identity, source key, and URL remain fixed. The caller must retain it in later
accepted writes. Additional appearances do not change that consumer identity.
Existing consumers continue to match the exported primary URL and optional
`apply_url`; secondary appearance URLs are not added to that interface.

Current workplaces project to the existing `location` string as location names
joined with `, ` in location-ID order. Empty workplace links project to null.
`work_mode` accepts `remote`, `hybrid`, `on-site`, or no value. Remote
projects to `remote:true`; hybrid and on-site project to `remote:false`; no
value projects to null. Geography remains separate from work mode. Current
status accepts the existing availability values in the table above. Export
uses one database transaction. `captured_at` describes snapshot capture;
record observation times describe the retained evidence. Separate list/show
calls may observe different committed states.

`--output` writes a private temporary file, syncs it, atomically replaces the
destination, and syncs its directory. It rejects the selected Cast database,
SQLite sidecars, and mutation lock as destinations, including aliases to those
paths. Stdout reads do not create an export file.

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

Reads return stored evidence without refreshing it. Cast has no source
retrieval interface. Use the supported ownership repair when older
associations need correction. Do not infer complete source coverage from an
absent error or repair database rows by hand.

CLI output, exports, and diagnostic captures remain private caller-owned data.
They can contain search interests, source locators, and posting text. Reads
send no information to remote systems and grant no authority to contact an
employer or start a downstream workflow.

Read contract 3, list/search/status schema 2, export schema 1, database
schema, and Cast package version are distinct. No maximum database size,
export size, read-latency service level, or future deprecation window is
promised. These reads rely only on local retained Cast state; external source
readiness is not a prerequisite for reading stored results.

# Inspect the discovery library

Use Cast's local read interfaces to inspect stored records:

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

Select independent state with the global `--state-dir PATH` option. Reads do
not collect sources, start an agent, contact anyone or create downstream state.
They require supported initialized Cast state.

The schema-one JSON snapshot contains `schema_version`, `snapshot_revision`,
`captured_at`, `companies`, `jobs`, `source_health` and query `coverage`.
Export uses one database transaction to return a consistent view. Separate
list/show calls may observe different committed states. `snapshot` is an alias
for `export`. Output uses JSON; the commands also accept an explicit `--json`
flag. `--output` writes a private temporary
file, syncs it, atomically replaces the destination and syncs its directory.
It rejects the selected Cast database, its SQLite sidecars and its mutation lock
as destinations, including aliases to those paths.

Company and job IDs are opaque stable Cast identities; record revisions track
stored changes. Source-native IDs are scoped to their source namespace.
Domain fields, careers URLs and source locators are included in the records.
Website-domain and ATS-tenant identities can describe the same real employer;
the same name alone does not merge them. ATS sources are owned by their canonical
provider/tenant identity. A corrected ownership association may change a job's
`company_id` and revision while preserving its job ID.

Reads and exports include all retained jobs. Collection applies no title
substring requirement. Job counts describe retained records, not all postings
available from external sources. Consumers choose jobs by title, seniority,
location or other preferences.

Job records retain Cast observation dates, source-supplied posting dates and a
recorded availability status. Collection records retain the latest attempt and
last successful observation separately. Third-party discovery records `unknown`.
The adapter assigns `employer_ats` or `employer_jsonld_owned` attribution using
its provider/tenant and employer-root URL matching rules. Shared recruiting
hosts require their own adapter. Ownership reconciliation applies these rules
to older rows and sets older `employer_jsonld` and shared-host rows to `unknown`.
An employer-source observation records `listed` or the source's explicit
`unlisted` flag. Absence from a completed employer-source scan records `missing`;
at least two such scans and 24 hours since the first absence record
`presumed_closed`. Failed or partial scans do not add missing observations.
An empty list contains no stored record matching the selected query and limits.
Relevance reasons record discovery matches. Posting descriptions are excerpts
limited to 1,200 characters, with a fingerprint for comparing descriptions.
The source locator supports a separate fetch when a consumer needs posting text.

`search` matches substrings in company names, domains, job titles and
descriptions. Matching ignores case. `unresolved` shows companies without
domains and sources whose status is not a successful
collection/resolution outcome. `status` reports counters, usage and the last run.

A consumer should use stable IDs and record revisions to compare snapshots.
It owns its consumed state, selected or dismissed jobs, packets, applications
and sent-message history. Cast exports stored companies, jobs and source metadata for
those workflows. This release has no durable change-feed acknowledgement or
consumer-retention protocol.

Use the CLI or Cast's provider-owned Rust API. Direct SQLite reads and writes
are not supported integration surfaces. Keep snapshots private: they can
contain search interests, source locators and extracted posting text. Reads
return stored records without making network requests.

## Output selection

List and search return schema-two pages with `snapshot_revision`, compact
items and `has_more`. The default limit is 20 items. Use `--limit` with a
positive integer to change it.

Job rows contain stable ID and revision, company, title, location, remote
eligibility, recorded availability and `last_seen_at`. Search adds
`matched_field` and a marked excerpt of at most 240 Unicode characters.

Status schema 2 returns counts, budgets, usage, the last run and collection
summaries for sources and queries. Show and export return full records, source
metadata and collection outcomes. The export snapshot still uses schema 1.

# Inspect the discovery library

Use Cast's local read interfaces to inspect what collection actually observed:

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
Export uses one database transaction so the consumer receives one consistent
view. Separate list/show calls may observe different committed states. `snapshot` is
an alias for `export`; JSON is the normal output form, and accepted `--json`
flags make the caller intent explicit. `--output` writes a private temporary
file, syncs it, atomically replaces the destination and syncs its directory.
It rejects the selected Cast database, its SQLite sidecars and its mutation lock
as destinations, including aliases to those paths.

Company and job IDs are opaque stable Cast identities; record revisions expose
retained changes. Source-native IDs are meaningful only in their source
namespace. Similar names and job titles are not independently verified identity
matches. Domain evidence, careers URLs and source locators remain visible.
Website-domain and ATS-tenant identities can describe the same real employer;
the same name alone does not merge them. ATS sources are owned by their canonical
provider/tenant identity. A corrected ownership association may change a job's
`company_id` and revision while preserving its job ID.

Cast observation dates, external posting dates, availability and collection
health mean different things. A recently attempted failing source is not fresh.
Third-party leads have `unknown` availability until employer/ATS observation.
Authoritative evidence uses `employer_ats` or `employer_jsonld_owned` with a
matching employer root homepage URL. Shared recruiting hosts require their own
adapter; an older owned marker on such a host does not establish employer
ownership. The supported ownership repair can quarantine these observations and
older unproven `employer_jsonld` evidence as unknown.
`listed` and explicit `unlisted` reflect that employer source. Absence from a
complete authoritative scan records `missing`; at least two such scans and 24
hours since the first absence are required for `presumed_closed`. Failed or
partial scans do not advance this absence policy.
An empty list or absent employer means only that selected retained state has no
matching record; collection coverage may be incomplete. Stored relevance reasons
are discovery signals, not a personal fit score. Posting descriptions are
excerpts limited to 1,200 characters. The description fingerprint cannot
reconstruct omitted text; downstream consumers must refresh the external source
when they need full current requirements.

`search` performs case-insensitive substring matching on company name/domain
and job title/description; it makes no semantic-confidence claim. `unresolved`
shows companies lacking domains and sources whose status is not a successful
collection/resolution outcome. `status` reports counters, usage and the last run.

A consumer should use stable IDs and record revisions to compare snapshots.
It owns its consumed state, selected or dismissed jobs, packets, applications
and sent-message history. Cast exports facts and evidence and does not absorb
those decisions. This release has no durable change-feed acknowledgement or
consumer-retention protocol.

Use the CLI or Cast's provider-owned Rust API. Direct SQLite reads and writes
are not supported integration surfaces. Keep snapshots private: they can
contain search interests, source locators and extracted posting text. Reads
make no network request and cannot establish current external truth.

## Output selection

List/search return schema-two pages with snapshot_revision, compact items and has_more; positive --limit defaults to 20. Job rows expose stable ID/revision, company, title, location/remote eligibility, availability and last_seen_at successful observation. Search adds matched_field and a marked excerpt of at most 240 Unicode characters. Status schema 2 returns counts, budgets/usage, last run and failing/incomplete source and query summaries. Show/export preserve full records, evidence and snapshot coverage; exported snapshot schema remains 1.

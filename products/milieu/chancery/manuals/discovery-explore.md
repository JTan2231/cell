# Milieu records and read handoff

Milieu owns company and job identities, current revisions, source metadata, and
read projections. Local callers and downstream products can inspect retained
fields or export one consistent snapshot. Reads require supported initialized
state and start no collection, agent, remote request, or downstream workflow.

The consumer handoff retains its public Rust read types and schema-one export.
Read `milieu.state` for supported database schemas, initialization, local status,
and retired administrative commands. Milieu performs no collection.

## Read interfaces

```sh
milieu companies list
milieu jobs list
milieu sources list
milieu company show COMPANY_ID
milieu job show JOB_ID
milieu search "infrastructure"
milieu export --json
milieu export --output /absolute/private/snapshot.json
```

Select independent state with global `--state-dir PATH`. Output is readable
text by default. Lists and search print compact rows and report when more
results exist. Show prints full retained fields, including posting text.

Use global `--json` before or after the command for the machine interface.
List/search, show, and export success schemas remain unchanged. `snapshot` is
an alias for `export`. With `--output`, the snapshot file remains JSON in both
modes; the flag selects the stdout receipt format. A stdout export prints a
readable snapshot by default or the complete JSON snapshot with `--json`.

The provider-owned Rust read surface is `milieu::store::Store::open(&Path)` and
`Store::snapshot()`, returning `milieu::models::Snapshot`. Use Milieu's exported
types. Direct SQLite reads and writes are not supported integration surfaces.

## Retained record meanings

A company is an accepted company identity. A job refers to its hiring employer
through `employer_id`. A source can independently refer to its operator through
optional `operator_id`. Source operator and job employer can be different
companies. Matching names alone do not merge company identities.

A job is one accepted current record, with workplace links and one or more
source appearances. Resolve duplicate matching and conflicting source values
before writing it. The public `company_id` is its `employer_id`. It is not a
recommendation, application, or hiring outcome. Relevance reasons describe
supplied discovery signals. Reads include all retained jobs without a title
substring requirement; consumers choose jobs by title, seniority, location,
or other preferences.

A source is a named board or feed with a URL and optional operator. Its
exported `company_id` retains the separate compatibility association used by
source enrollment. That association does not establish a job employer or
source operator. Its exported enabled flag, status, timestamps, cursor, and
note remain supplied compatibility metadata. No collector acts on them.

Company, job, and source IDs are opaque stable Milieu identities. Source-native
IDs are scoped to their source namespace. Current record revisions are
computed from exported material fields, excluding `last_seen_at`. Milieu retains
only each entity's current revision, without a revision-history table.
Secondary appearance changes and changes between hybrid and on-site can
preserve the export revision while advancing the snapshot revision.

Records retain company domains, website URLs, company identity aliases, source
locators, and accepted job fields. Descriptions are stored as supplied. A
source locator supports a separate fetch when a consumer needs posting text;
reading the locator causes no fetch. Milieu retains accepted fields instead of
whole provider responses and HTML documents.

## Availability and freshness

Milieu preserves supplied timestamps and evidence without creating observation
history. Company/job `first_seen_at` and `last_seen_at` normalize to UTC with
nine fractional-second digits for chronological string comparison. Writing or
exporting an accepted job does not establish that its source was observed at
the write or capture time. Source-supplied publication and update dates use
separate fields.

Current `status` projects to exported `availability` and accepts `unknown`,
`listed`, `unlisted`, `missing`, or `presumed_closed`. These values record the
accepted availability assertion. Milieu does not scan, count missing postings,
or establish fresh availability from those values.

Compatibility fields retain supplied attribution, missing-scan counts, first
absence times, parser versions, fingerprints, and source collection dates.
They describe supplied evidence. Their presence does not establish new
collection or fresh source health. Freshness is age of successful evidence,
separate from the latest attempt. No command refreshes evidence or establishes
current external availability.

## Output selection

With `--json`, list and search return schema-two pages with
`snapshot_revision`, compact `items`, and `has_more`. The default limit is 20
items. `--limit` accepts a positive integer. An empty page has no stored record
matching the selected query and limits; it does not establish that no external
jobs exist.

Job rows contain stable ID and revision, company, title, location, remote
eligibility, recorded availability, and `last_seen_at`. Search matches
substrings in company names, domains, job titles, and descriptions, ignoring
case. Its rows add `matched_field` and a marked excerpt of at most 240 Unicode
characters. Show and export return full records and source metadata.

Counts describe retained records. They do not count all postings available
from external sources. The local status and readiness interfaces belong to
`milieu.state`.

## Consistent export

The schema-one snapshot contains `schema_version`, `snapshot_revision`,
`captured_at`, `companies`, `jobs`, `source_health`, and query `coverage`. It is
one consistent current read view with stable identities, current revisions,
compatibility fields, and supplied evidence. `coverage` is an empty array:
current state has no query coverage or collection history. The `source_health`
name remains for compatibility and carries retained source metadata.

Each job exports one primary source appearance through `source_id`,
`source_key`, and `url`. Additional appearances remain in the current store;
the existing snapshot does not expose all source links. The caller supplies
the primary appearance when accepting a new job. Its source identity, source
key, and URL remain fixed, and later accepted writes must retain that tuple.
Additional appearances do not change the consumer identity. Consumers continue
to use the primary URL and optional `apply_url`; secondary appearance URLs are
not added to the interface.

Current workplaces project to the `location` string as names joined with
`, ` in location-ID order. Empty workplace links project to null. `work_mode`
accepts `remote`, `hybrid`, `on-site`, or no value. Remote projects to
`remote:true`; hybrid and on-site project to `remote:false`; no value projects
to null. Geography remains separate from work mode.

Company/job exports preserve supplied evidence, compatibility dates,
compensation, geographic eligibility, publication semantics, and current
entity revisions. Export uses one database transaction. `captured_at`
describes snapshot capture; record dates describe supplied evidence. Separate
list/show calls may observe different committed states.

`--output` writes a private temporary file, syncs it, atomically replaces the
destination, and syncs its directory. It rejects the selected Milieu database,
SQLite sidecars, and mutation lock as destinations, including aliases to those
paths. Stdout reads do not create an export file.

Consumers use stable IDs and record revisions to compare snapshots. They own
consumed state, selected or dismissed jobs, packets, applications, CRM cases,
and sent-message history. Milieu has no durable change-feed acknowledgement or
consumer-retention protocol.

## Failure, access, and compatibility

Operational errors exit 1. They use a text diagnostic on stderr by default.
With `--json`, stderr contains one schema-one object with `ok:false` and
`error.detail`. Invalid command syntax uses Clap's text diagnostic on stderr
and exit 2 in both modes. The operational `status-snapshot --json` protocol is
separate from ordinary command output; read `milieu.state` for its boundary.

Reads return stored evidence without refreshing it. Milieu has no source
retrieval interface. Do not infer complete source coverage from an absent
error or repair database rows by hand. Read `milieu.state` for supported storage
and recovery.

CLI output, exports, and diagnostic captures remain private caller-owned data.
They can contain source locators and posting text. Reads send no information
to remote systems and grant no authority to contact an employer or start a
downstream workflow.

Read contract 3, list/search schema 2, export schema 1, database schema, and
Milieu package version are distinct. Contract 3 continues to describe the
preserved consumer handoff and record-read types. State contract 3 describes
the local CLI and storage retirements in Milieu 0.6.0. No maximum database size,
export size, read-latency service level, or future deprecation window is
promised. These reads rely only on local retained Milieu state.

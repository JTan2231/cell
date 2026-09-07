# Explore the CRM library

Use this capability to find an employment-related case or inspect an
exact immutable revision and its history. These commands read only the selected
CRM database. They launch no worker or model, call no source or network, and
create no Nucleus job.

## Select the library

The default database is:

```text
~/Library/Application Support/CRM/crm.db
```

Use global `--database PATH` or `CRM_DATABASE` for another library.
The command-line option wins. A relative selection resolves against the current
working directory. Missing, foreign, or unsupported schemas are refused; reads
never initialize or migrate storage.

Run `crm doctor` against the same selection when schema, integrity, permission,
or Nucleus/toolset readiness is uncertain.

## List current cases

```sh
/Users/joey/.local/bin/crm case list --limit 50
```

List returns summaries of current revisions in deterministic order. Each result identifies its
case and current revision and includes title, stage, summary, and nullable
advisory. A non-null advisory is rendered prominently. Limits bound output and
do not alter the stored library.

## Search current cases

```sh
/Users/joey/.local/bin/crm search "voice AI hiring manager" --limit 20
```

Search matches literal substrings in stored titles and current revision
material and orders results by case update time and identity. A no-match
result contains no current case head matching the query within the selected
limits.

## Inspect a current or historical revision

```sh
/Users/joey/.local/bin/crm case show CASE_ID
/Users/joey/.local/bin/crm case show CASE_ID --revision 1
/Users/joey/.local/bin/crm case history CASE_ID
```

If you omit `--revision`, CRM returns the current committed revision. If you
supply a positive revision number, CRM returns that exact immutable revision.
History returns the newest revisions first. It defaults to 20 summaries with
`has_more`; increase `--limit` for more.
Use `case show --revision N` for the complete snapshot behind a history row.

Each revision is a full snapshot containing:

- complete Markdown;
- one of `research`, `warranted`, `contacted`, `connected`, `helped`, or
  `closed`;
- nullable advisory; and
- summary.

The revision's `source_update_id` can be passed to `crm update show` to inspect
its update/delivery and Nucleus identities. Version 0.3 has no supported
raw-delivery, persisted-request, or mailbox-receipt show/export command; direct
SQLite reads are unsupported. Historical output returns the selected stored
revision.

## Advisory and authority

Human output prefixes a present warning with
`ATTENTION — STEWARD ADVISORY (NON-BLOCKING)`. JSON carries `attention: true`
and the advisory text so a downstream consumer can render it visibly. The
advisory is part of the evidence and must not be hidden, but it never blocks
reading, telling, stage changes, or any caller-owned action.

CRM owns the existence, ordering, content digest, stage, summary, advisory and
stored correlations of its revisions. Each revision contains the case narrative
produced from its supplied delivery and previous revision. Deliveries retain
their supplied source references.

## Machine output and privacy

Use global `--json` for the machine envelope:

```json
{"ok":true,"data":{"type":"..."}}
```

Identifiers are opaque. Exact reads return every field of the selected stored
revision; list and search return current heads up to the chosen limits. No
wall-clock latency or database-size service level is promised.

CRM output can expose private contact, employment, interaction, source,
summary, advisory, and Nucleus-correlation data. Terminal display and redirected
output are caller-controlled disclosure surfaces.

## Rust callers

The provider crate exports `crm::api`: supported request and response
types, provider-owned envelope decoding, and an explicit-executable CLI client.
Use these types at imports and convert only to caller-local domain values.
The client performs the same operations under this contract and never adds
retry or authorization. See `crm/docs/rust-api.md`; the Rust structs and
enums define the interface without a separate declaration layer.

## Output selection

Case list, search and history default to 20 results with `has_more`. Use
`--limit` with a positive integer for more. History returns case ID, revision,
stage, summary, complete advisory and attention, and `created_at`.
Search returns current identity, revision, stage, summary, advisory and
`matched_field`. It adds a marked excerpt of at most 240 Unicode characters
around the match. Case show returns the exact full revision.

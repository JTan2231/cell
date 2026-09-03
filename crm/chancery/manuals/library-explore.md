# Explore the CRM library

Use this capability to find an existing employment-oriented case or inspect an
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

List returns deterministic current-head summaries. Each result identifies its
case and current revision and includes title, stage, summary, and nullable
advisory. A non-null advisory is rendered prominently. Limits bound output and
do not alter the stored library.

## Search current cases

```sh
/Users/joey/.local/bin/crm search "voice AI hiring manager" --limit 20
```

Search is deterministic lexical retrieval over stored titles and current
revision material. It is not an embedding or model search. Rank is retrieval
behavior, not confidence, source freshness, contact worthiness, or proof that a
case applies to the present request.

A no-match result says only that retained current heads did not satisfy the
query and limit. It does not establish that no useful person, company, posting,
location, or relationship exists.

## Inspect a current or historical revision

```sh
/Users/joey/.local/bin/crm case show CASE_ID
/Users/joey/.local/bin/crm case show CASE_ID --revision 1
/Users/joey/.local/bin/crm case history CASE_ID
```

Omitting `--revision` selects the current committed head. Supplying a positive
revision selects that exact immutable snapshot. History returns the complete
retained lineage in case-local order.

Each revision is a full snapshot containing:

- complete Markdown;
- one of `research`, `warranted`, `contacted`, `connected`, `helped`, or
  `closed`;
- nullable advisory; and
- summary.

The revision's `source_update_id` can be passed to `crm update show` to inspect
its update/delivery and Nucleus identities. Version 0.1 has no supported
raw-delivery, persisted-request, or mailbox-receipt show/export command; direct
SQLite reads are unsupported. Historical output does not imply current external
truth.

## Advisory and authority

Human output prefixes a present warning with
`ATTENTION — STEWARD ADVISORY (NON-BLOCKING)`. JSON carries `attention: true`
and the advisory text so a downstream consumer can render it visibly. The
advisory is part of the evidence and must not be hidden, but it never blocks
reading, telling, stage changes, or any caller-owned action.

CRM owns the existence, ordering, content digest, stage, summary, advisory, and
stored correlations of its revisions. It does not own the truth or freshness
of a caller-supplied source and does not independently observe contact,
connection, or help. Before relying on mutable evidence, reopen it through its
source.

## Machine output and privacy

Use global `--json` for the machine envelope:

```json
{"ok":true,"data":{"type":"..."}}
```

Identifiers are opaque. An exact revision is complete for that stored snapshot;
list and search cover only current heads up to the chosen limits. No wall-clock
latency, semantic recall, external source coverage, or database-size service
level is promised.

CRM output can expose private contact, employment, interaction, source,
summary, advisory, and Nucleus-correlation data. Terminal display and redirected
output are caller-controlled disclosure surfaces.

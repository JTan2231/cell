# CLI contract

CRM commands operate on one explicitly selected schema-one database. The
default is `~/Library/Application Support/CRM/crm.db`; global `--database`
takes precedence over `CRM_DATABASE`. A relative selected path resolves against
the current working directory. Ordinary commands never initialize or migrate a
missing database.

All commands accept global `--json`. Human output is intended for terminals;
JSON uses the stable envelope described below.

## Storage and readiness

```sh
crm [--database PATH] init
crm [--database PATH] doctor
```

`init` creates schema one when the selected file is absent. Repeating it against
an existing supported CRM database is idempotent; a foreign or unsupported
schema is refused. A symbolic link or other non-regular database target is
refused before SQLite opens or changes it. New database bytes are mode 0600 on
Unix. `doctor` checks schema identity and required tables, foreign keys, SQLite
integrity, secure database/sidecar permissions, and strict Nucleus/toolset
readiness without changing case domain state.

Nucleus unavailability does not make stored cases unreadable, but it prevents
new steward progress until readiness is restored. There is no direct agent
fallback.

## Cases

```sh
crm case new --title TITLE [INPUT] [--stage STAGE]
crm case list [--limit N]
crm case show CASE_ID [--revision N]
crm case history CASE_ID
```

`INPUT` is a regular non-symbolic UTF-8 Markdown file or `-` for standard
input, at most 1,048,576 bytes. The input path is transient and is not stored.
Omitting it generates this small suggested outline:

```markdown
# TITLE

## Current picture

## People

## Chronicle

## Open threads
```

The outline is editorial guidance, not a schema: supplied Markdown need not use
these headings, and CRM neither parses nor enforces them. The initial stage
defaults to `research`; the supported values are `research`, `warranted`,
`contacted`, `connected`, `helped`, and `closed`.

`new` commits the case and immutable revision one together. `list` returns
current case heads with deterministic ordering; list limits default to 20 and
must be from 1 through 1,000.
`show` returns the current revision unless `--revision` names a positive exact
revision. `history` returns the complete immutable revision lineage.

Every case read includes stage, summary, attention, and the advisory field.
Tell acknowledgments and every update surface also carry the relevant
revision's attention/advisory. Human output prefixes every non-null advisory with
`ATTENTION — STEWARD ADVISORY (NON-BLOCKING)`; JSON carries `attention: true`
and the advisory text so downstream consumers cannot silently discard it.

## Search

```sh
crm search QUERY [--limit N]
```

Search performs one case-insensitive literal-substring match over stored case
titles, current Markdown, and current advisory, ordered by most recently
updated case and then case identity. It is not semantic matching, source
verification, or a claim that a result warrants contact. Results identify the
exact current revision and include stage, summary, attention, advisory, and a
Markdown snippet. Limits default to 20 and must be from 1 through 1,000.

## Tell

```sh
crm tell CASE_ID INPUT [--name LABEL] [--source REF]
```

`INPUT` is required and is a regular non-symbolic UTF-8 free-form text file or
`-` for standard input, at most 1,048,576 bytes. CRM bounds and validates it
before storage, stores the exact text and digest in SQLite, and retains
optional `LABEL` and opaque
source reference `REF`. It does not retain the path, fetch `REF`, create a
content file, or send anything to a person.

In one transaction `tell` records the delivery and one queued update. After
commit it launches the hidden worker and immediately returns the queued update;
machine output includes its update and delivery IDs. It does not wait for
Nucleus or for a revised case. A launch or readiness failure leaves recoverable
durable work rather than rolling back accepted intake.

## Updates

```sh
crm update list [--limit N]
crm update show UPDATE_ID
crm update wait UPDATE_ID [--timeout SECONDS]
crm update resume UPDATE_ID
crm update retry UPDATE_ID
```

`list` and `show` expose CRM update state, case/delivery identity, frozen base
and applied revisions, exact Nucleus correlation, predecessor update, and
failure state without printing the persisted request or tool bodies. Update
states are `queued`, `running`, `applied`, `failed`, and `lost`; list limits
default to 20 and must be from 1 through 1,000.

Tell and retry acknowledgments plus update list/show/wait/resume/retry include
`attention` and advisory for the relevant case revision. An applied update uses
its applied revision; other updates use their frozen base when assigned and
otherwise the current head.

Version 0.1 has no public raw-delivery, persisted-request, or mailbox-receipt
show/export command. Case history plus update and Nucleus job identities are the
supported inspection path; direct SQLite access is unsupported.

`wait` observes until a failed/lost update is final or an applied update also
has a retained terminal Nucleus observation. Once on entry, if domain or
runtime settlement still needs work, it activates queue drain or same-update
recovery and then only polls. `--timeout SECONDS` defaults to 1,200; timeout
returns an error without making the update terminal. `resume` synchronously
processes queued work or resumes the same recoverable running or
applied-but-unsettled update and pending mailbox call; it does not create a new
update. `retry` is accepted only for `failed` or `lost` work. It records a
successor update reusing the same immutable delivery row/text, with new
requester/job identities and a retained `retry_of` link.

Nucleus completion without a committed revision is not CRM success. A revision
that committed before a later runtime failure remains CRM success and cannot be
retried into a duplicate revision. Update output exposes result-post and
runtime state/detail, and any post-commit failure remains a diagnostic rather
than a blocking state.

## Failures and machine output

Commands return nonzero for invalid UTF-8 or input bounds, missing or
unsupported storage, unknown identity, invalid stage, failed integrity,
conflicting replay, stale base, unavailable required readiness, or an invalid
resume/retry transition. Validation failures commit no partial case revision.
A successfully recorded `tell` remains successful even if its post-commit
worker launch cannot progress immediately.

After successful argument parsing, machine output uses a common envelope. Success is
`{"ok":true,"data":{"type":"..."}}`; failure is
`{"ok":false,"error":{"code":"...","message":"..."}}`.
Record identifiers are opaque even when examples use readable prefixes.

When `update wait`, `update resume`, or `update retry` fails after consuming a
known update, the failure envelope also includes
`"context":{"type":"update","update":{...}}`. That update view carries the
relevant `attention` and advisory even though the command remains a nonzero
operational failure. Human stderr prints the same nonblocking advisory banner
before the error.

Advisories are data, not errors: a non-null advisory never changes an otherwise
valid command's exit status or authorizes CRM to refuse the operation.

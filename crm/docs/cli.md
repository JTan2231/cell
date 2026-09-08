# Commands and records

CRM commands operate on one explicitly selected schema-two database. The
default is `~/Library/Application Support/CRM/crm.db`; global `--database`
takes precedence over `CRM_DATABASE`. A relative selected path resolves against
the current working directory. Ordinary commands never initialize or migrate a
missing database.

All commands accept global `--json`. Human output is intended for terminals;
JSON uses the stable envelope described below.

## Storage and readiness

```sh
crm [--database PATH] init
crm [--database PATH] migrate --backup PATH
crm [--database PATH] doctor
```

`init` creates schema two when the selected file is absent. Repeating it against
an existing supported CRM database is idempotent; a foreign or unsupported
schema is refused. A symbolic link or other non-regular database target is
refused before SQLite opens or changes it. New database bytes are mode 0600 on
Unix. `doctor` checks schema identity and required tables, foreign keys, SQLite
integrity, secure database/sidecar permissions, and strict Nucleus/toolset
readiness without changing case domain state.

`migrate --backup PATH` upgrades schema one to schema two explicitly; it never
imports profile content or launches a worker. Stop new CRM work and let active
workers settle. Migration refuses a live worker PID, running work, or applied
work without terminal runtime evidence, but preserves queued work. It creates
a private, independently readable SQLite backup at a new path before changing
schema.

The parent must be an existing non-symbolic private directory, with no
group/other permissions on Unix. A relative backup path resolves against the
current working directory. Existing backup destinations, source/sidecar paths,
and destinations with existing SQLite sidecars are refused. Schema changes and
integrity validation are transactional. Repeating against schema two returns
unchanged without creating a backup.

JSON returns `type: "migrated"`, the database path, `backup` (absolute path or
null for unchanged schema two), `changed`, `from_schema_version`, and
`schema_version`. After an ambiguous failure, inspect schema before retrying;
a failed backup may leave its destination, so use a fresh path after
inspecting it. See [migration and
rollback](data-model.md#initialization-and-migration).

Nucleus unavailability does not make stored cases unreadable, but it prevents
new steward progress until readiness is restored. There is no direct agent
fallback.

## Profile entries

```sh
crm profile new --title TITLE INPUT
crm profile list [--limit N]
crm profile show PROFILE_ID
crm profile update PROFILE_ID --title TITLE INPUT
```

New and update require `INPUT`: a regular UTF-8 Markdown file or `-` for standard
input. Files must not be symbolic links. Input is limited to 1,048,576 bytes. Empty bodies are
allowed. CRM preserves the body exactly. Titles are trimmed and must contain
1 through 1,000 UTF-8 bytes after trimming. Titles need not be unique. File
paths are transient; CRM does not retain, move, delete, or synchronize them.

`new` creates an opaque stable ID and commits one row with `title`, `body_md`,
and `updated_at` (an RFC 3339 UTC string). `update` replaces the entire title
and body atomically, retains the ID, and updates the timestamp. It has no
revision guard or history; the last committed update wins. There is no partial
edit, deletion, or automatic duplicate detection.

`list` orders entries by `updated_at` descending and then ID. Human output
and JSON show only ID, title and timestamp. Limits default to 20 and must be
positive. Results include `has_more`; increase `--limit` for more. `show` returns all
four fields of one current entry. Each read observes current committed state;
separate commands are not a shared snapshot. `search` still searches cases
only.

Profile commands invoke no model, Nucleus job, network, or worker. They neither
change cases nor automatically supply context to the steward. Rules and caveats
in Markdown are retained text for the caller to interpret. New and update
validate the full input before committing; an unknown ID or invalid input
causes no partial profile write. Retrying `new` can create a duplicate; inspect
`list`/`show` after an ambiguous result before deciding to create another row.

## Cases

```sh
crm case new --title TITLE [INPUT] [--stage STAGE]
crm case list [--limit N]
crm case show CASE_ID [--revision N]
crm case history CASE_ID [--limit N]
```

`INPUT` is a regular UTF-8 Markdown file or `-` for standard input. Files must
not be symbolic links. Input is limited to 1,048,576 bytes. CRM does not store the input path.
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
current case revisions in deterministic order. The default limit is 20 and
must be positive. `has_more` indicates whether increasing `--limit` returns more.
`show` returns the current revision unless `--revision` names a positive exact
revision. `history` returns newest-first revision summaries: case ID, revision,
stage, summary, advisory/attention and recorded time. It defaults to 20 with
`has_more`; increase `--limit` to read more history.

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
updated case and then case identity. Results identify the exact current
revision and include stage, summary, attention, advisory, and a marked excerpt
of at most 240 Unicode characters around the match, with `matched_field`
(`title`, `advisory` or `markdown`) and `excerpt: true`. Limits default to 20 and
must be positive; `has_more` indicates whether increasing `--limit` returns more.

## Tell

```sh
crm tell CASE_ID INPUT [--name LABEL] [--source REF]
```

`INPUT` is required: a regular UTF-8 text file or `-` for standard input. Files
must not be symbolic links. Input is limited to 1,048,576 bytes. CRM validates it
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
default to 20 and must be positive; `has_more` indicates whether increasing `--limit` returns more.

Tell and retry acknowledgments plus update list/show/wait/resume/retry include
`attention` and advisory for the relevant case revision. An applied update uses
its applied revision; other updates use their frozen base when assigned and
otherwise the current head.

CRM has no public raw-delivery, persisted-request, or mailbox-receipt
show/export command. Case history plus update and Nucleus job identities are the
supported inspection path; direct SQLite access is unsupported.

`wait` observes until a failed/lost update is final or an applied update also
has a retained terminal Nucleus observation. Once on entry, if domain or
runtime settlement still needs work, it activates queue drain or same-update
recovery and then only polls. `--timeout SECONDS` defaults to 1,200; timeout
returns an error without making the update terminal.

`resume` synchronously processes queued work or resumes the same recoverable
running or applied-but-unsettled update and pending mailbox call; it does not
create a new update. `retry` is accepted only for `failed` or `lost` work. It
records a successor update reusing the same immutable delivery row/text, with
new requester/job identities and a retained `retry_of` link.

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

## Compact output

Profile new/update return `type: profile_receipt` with ID, title and updated
time; profile show remains `type: profile_entry` with exact Markdown.
Case new returns `case_created` with case ID, revision, stage, summary,
advisory/attention and recorded time. It does not echo Markdown or its hash.
Every profile/case/update list and search includes `has_more`. Read the selected
case revision or profile with `show` for full content. Advisories remain complete
and non-blocking on every revision-consuming view. The Rust client exposes
`ProfileSummary` and `RevisionSummary` for the compact results.

## Rust callers

Use the provider-owned `crm::api::Client`, `Request`, and `Data` for typed
CLI calls. The client preserves the documented envelope and successful stderr
diagnostics, performs no automatic retries, and uses the same command effects
and recovery rules. See [Rust interface](rust-api.md) for the exported boundary.

## Deployment maintenance

See [installation and maintenance](system-installation.md#coordinated-deployment-maintenance)
for admission holds, drain, controlled migration, and deployment verification.

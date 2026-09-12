# Store and read career profile entries

Use this capability for reusable career vignettes, statements, preferences, or
other profile material that the caller wants to retrieve and edit
independently. Each entry is one current Markdown body with an opaque stable
ID, title, and write timestamp. Profile entries are separate from cases and
immutable case revisions.

## Select the library

The default database is `~/Library/Application Support/CRM/crm.db`. Global
`--database PATH` overrides `CRM_DATABASE`; relative selections resolve against
the current working directory. Profile commands require an initialized
schema-two CRM database. Missing, foreign, or unsupported storage is refused;
these commands never initialize or migrate it. Schema migration belongs to
`crm.steward.operate` and is an explicit separate effect.

## Create or replace an entry

```sh
/Users/joey/.local/bin/crm profile new --title TITLE INPUT
/Users/joey/.local/bin/crm profile update PROFILE_ID --title TITLE INPUT
```

`INPUT` is required: a regular UTF-8 file or `-` for standard input. Files must
not be symbolic links. The body may be empty and must not exceed 1,048,576
UTF-8 bytes. CRM preserves it exactly as SQLite `TEXT`, including line breaks,
headings, caveats, and disclosure guidance. It neither parses nor rewrites the
Markdown. The title is trimmed and must then contain 1 through 1,000 UTF-8
bytes. Titles need not be unique.

New commits one row with a generated identity. Update atomically replaces the
complete title and body of the selected row, keeps its identity, and updates
`updated_at`. This timestamp records CRM write time as an RFC 3339 UTC string.
There is no partial edit,
revision precondition, merge, or history. The last committed replacement wins.
Keep any desired prior content before replacing it.

Validation failure or an unknown update ID commits no partial profile change.
Creation has no idempotency key or duplicate check: after an ambiguous result,
inspect the current entries before deciding to create another. A failed process
or lost output does not prove that a write failed to commit.

## List and read

```sh
/Users/joey/.local/bin/crm profile list --limit 50
/Users/joey/.local/bin/crm profile show PROFILE_ID
```

List orders entries by `updated_at` descending and then ID. Human output
shows ID, title, and timestamp; JSON returns the same selection fields and
`has_more`. Limits default to 20 and must be positive. Show returns the
complete current entry, including `body_md`. Separate calls do not share a
snapshot. No profile search or delete command exists; `crm search` continues
to search cases only.

All commands support global `--json`. Success uses
`{"ok":true,"data":{"type":"..."}}`; failure uses
`{"ok":false,"error":{"code":"...","message":"..."}}` and a nonzero exit.
IDs are opaque. Rust callers use the exported `crm::api` request, payload,
and CLI-client types with the same effects and no automatic retry.

## Scope and privacy

These operations call no model, worker, Nucleus job, source, or network.
They do not change cases, automatically supply steward context, link entries
to cases, or enforce rules found in Markdown. The caller owns corrections,
interpretation, and any decision to supply selected text to another task.

Input files are transient transport. CRM does not retain their paths, move or
delete them, synchronize them, or create a parallel content tree. Storing an
entry leaves the source file and other workflows unchanged. CRM stores source
material and disclosure guidance as supplied Markdown. The database, backups,
terminal display and redirected output may
contain private identity, contact, employment, eligibility, or preference
information. Opening the database can enforce private database/sidecar
permissions and WAL mode; list and show make no domain mutation.

This capability grants no deployment, initialization, migration, source-file
deletion, case change, external disclosure, or contact authority. It promises
no database-size service level, execution latency or future retention/deprecation
window.

## Output selection

Profile new and update return `profile_receipt` with ID, title and `updated_at`.
List returns these compact entries with `has_more`; show returns exact Markdown.
The default limit is 20. Use `--limit` with a positive integer to change it.
The `ok`/`data` envelope and schema-two profile content retain their meanings.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.

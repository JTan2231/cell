# Record and read a ledger notepad

Clew retains supplied notes and optional free-text status in an append-only
ledger. A note can stand alone or belong to a named thread. Optional references
link an entry to stable identities outside Clew. Clew records what the caller
reports. It does not verify work, infer completion, or run a model.

## Append an entry

```sh
clew record --id sla-start --thread 'SLA implementation' \
  --notes 'Started the implementation.'
clew record --id sla-progress --thread 'SLA implementation' \
  --notes 'Timeout handling is finished. Reporting remains.'
clew record --id sla-done --thread 'SLA implementation' \
  --status done --notes 'Finished the implementation.'
clew record --id reminder --notes 'Ask about the migration window.'
```

Supply a stable write ID and at least one nonblank `--notes` or `--status` value.
Notes and status retain supplied text. An omitted field is null. Status has no
required vocabulary or transition sequence. `recorded_at` is the UTC RFC3339 time
Clew recorded the entry. Put supplied historical dates in notes.

`--thread NAME` selects an existing thread or creates it with the entry in one
transaction. A thread holds only its stable Clew ID and unique name. Names are
nonblank and case-sensitive. Use the same exact name for later appends. Omitting
`--thread` creates a standalone entry. There is no `--continue` argument or
required first-entry reference. Clew does not assign thread status or completion. Threads have no rename,
merge, or delete operation.

## Attach an external reference

```sh
clew record --id sla-commit --thread 'SLA implementation' \
  --notes 'The implementation is in this commit.' \
  --ref repository.commit 'repository-id/commit-id'
clew record --id job-context --notes 'This posting explains the requirement.' \
  --ref cast.job JOB_ID
```

Repeat `--ref NAMESPACE EXTERNAL_ID` to attach several links. An external
reference identifies something outside Clew. Both values must be nonblank and
retain their exact supplied spelling. Repeated identical links are deduplicated;
link order does not change write identity. The namespace and external ID form
its identity. Clew retains these opaque values without resolving them, fetching
content, or checking that the external object exists. Reference records contain
no local subject title. The entry text and optional thread name identify the
local work.

Entries can have zero references. Several entries can link to the same external
identity without belonging to the same thread. A thread can contain entries
with different references or no references. Thread membership and external links
are separate facts.

A plain `--ref cast.job JOB_ID` is a link. It neither creates an application
report nor changes application status, `clew list`, or the daily email. Use the
explicit `--cast-job JOB_ID` form for an application report. Read
`chancery show clew.application.track` for first-job admission, legacy aliases,
and the application view. Generic entries require no Cast or Platter read.

## Read entries and threads

```sh
clew search 'SLA'
clew thread 'SLA implementation'
clew entry sla-done
```

Search matches retained entry text, status, thread names and IDs, entry IDs, and
external reference identifiers through case-insensitive local substring matching.
Supply a nonblank query; Clew does not rank matches. It includes superseded and retracted
entries as historical clues. Search does not interpret a natural-language
question, select a most likely thread, or establish that work was completed.
The conversational agent can search, retrieve the matching thread, and answer
from the recorded statements and their corrections.

Thread reads return `thread`, the latest active nonnull `status`, its supplying
`status_entry_id`, and complete `history` in ledger sequence order. A notes-only
append does not change that status. If no active record supplies status, it is
null. Standalone entries are available through entry reads and search. Entry
reads select an exact Clew write ID and return that entry. Search returns the
matching entries. Each history entry exposes `replaces` and its immediate
`superseded_by` ID; an active record has kind `record` and no superseding entry.
An unknown entry or thread fails explicitly. Reads return the complete selected
records without paging or automatic pruning. Each generic read uses one ledger
snapshot; successive commands can observe different appends.

Commands print readable text by default. Search prints a summary row for each
matching entry. Entry and thread reads include the full selected text. Use
`--json` for machine reads and writes. JSON success retains `ok`,
`schema_version: 3`, and `data`. Human errors go to stderr. JSON errors retain
`ok: false` and `error.detail` on stdout, with a nonzero exit. Entries expose `sequence`, `id`, `recorded_at`, `kind`, `status`, `notes`,
`replaces`, `thread`, and `references`. Thread is null or `{id, name}`. Each
reference has `namespace`, `external_id`, and `role`, which is `link` or
`application_report`. Search and entry reads also expose `superseded_by`.
Sequence orders committed appends. Entry IDs and thread IDs are distinct Clew identities;
external IDs belong to their stated namespaces.

## Correct or retract an entry

```sh
clew record --id sla-correction --replaces sla-done \
  --status 'in progress' --notes 'Reporting still needs work.'
clew retract reminder --id reminder-retraction \
  --notes 'The migration window is no longer relevant.'
```

A correction is a new `record` that replaces one active record. It supplies
complete new notes, status, and reference associations. Omitted content and links
are not copied. A correction inherits its target's thread. Omitting `--thread`
is sufficient; an explicitly different thread is rejected. This keeps retained
history together without making a replacement an unrelated thread append.

A `retraction` removes its active target from current calculations and retains
its explanatory notes and correction relationship. It preserves the target's
thread and reference associations. It does not erase the target or other entries. A retraction
cannot itself be retracted. Correct an already replaced record by selecting its
active replacement. Restore a retracted statement by appending a new record.

## Retry and recover writes

Choose the write ID before invocation. IDs contain 1 through 128 ASCII letters,
digits, dots, underscores, or hyphens. Reuse the original ID and identical
arguments after an uncertain result. An exact retry returns the original entry,
even if a later correction superseded it. Different arguments under that ID
fail without appending. The retained write identity includes operation, content,
thread selection, references, and any explicit application-report target. Legacy
application aliases retain their original argument namespace after migration.

A write commits its entry, thread creation, and reference associations together
before returning success. SQLite serializes writes, uses synchronous FULL, and
waits up to five seconds for contention. Invalid correction targets, conflicting
IDs, or invalid inputs leave history unchanged. Updates and deletes of ledger
entries and associations are prohibited. Ordinary commands never initialize or
migrate state implicitly.

Use initialized ledger schema three at `~/.local/share/clew/ledger.sqlite3`, or
select an independent private ledger with global `--state-dir ABSOLUTE_PATH`.
The directory has mode 0700 and the database is a regular file with mode 0600.
Read `chancery show clew.state` for initialization, migration, backup, and
compatible recovery. Never repair an entry by editing SQLite.

Keep supplied notes, status, thread names, and links private. These operations
print content only to the local caller and make no network request. Authorized
application emails disclose only their selected application reports. Metadata
usage recording includes command identity and chat correlation, not content;
recording errors preserve the product result.

Full reads use memory proportional to retained history. There is no hard size,
latency, search-relevance, external availability, or future compatibility
promise. Clew does not run completion checks, apply migrations, interpret work,
contact external parties, or prune history.

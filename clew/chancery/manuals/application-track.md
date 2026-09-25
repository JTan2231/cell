# Record application status and notes

Clew stores what the user reports about a job retained by Cast. The
user supplies every status. The conversational agent finds the intended job
and resolves material ambiguity before writing. Clew accepts an exact job ID;
it does not interpret prose or choose a candidate.

## Find the job

```sh
clew find 'company, role, nickname or URL'
```

Find reads one complete retained Cast snapshot through `cast.discovery.explore`.
It searches job IDs, companies, roles, URLs, retained legacy references and Clew
notes. Notes from superseded records are included as search clues. HTTP URL
queries use supported ATS identity or Cast URL normalization. These local reads
make no network request. The two products are read in separate snapshots.

The result contains all `candidates`, matching
`retained_references_without_cast_record`, and `complete: true`. Each candidate
contains `cast_job_id`, `company`, `title`, `urls` and `tracked`. Tracked is true
when the job has active Clew history. Missing-record references are canonical
Cast job IDs. A missing Cast reader or failed read is an error, not a complete
empty result. Use `list` or `show` for retained Clew history while Cast is
unavailable. Search includes all retained jobs, not only a default result page.

Use the conversation and returned company, role and URLs to identify the intended
job. Ask the user when plausible choices remain. An unknown nickname requires
clarification. A URL with no retained match does not authorize collection.
Collect an explicitly selected supported public URL through Cast separately when
authorized. Clew does not collect jobs or require a Platter packet.

## Append the supplied report

```sh
clew record --cast-job JOB_ID --id WRITE_ID --status applied --notes 'Applied today.'
clew record --cast-job JOB_ID --id NEXT_WRITE_ID --notes 'Recruiter asked about availability.'
```

Supply at least one nonblank `--status` or `--notes`. Status is free text, with
no required sequence of transitions. Notes retain the supplied text. Omit a
field to store null. A notes-only entry leaves current status unchanged.

The first record for a job must resolve through the installed Cast snapshot
reader. An already recorded exact job can receive further reports while Cast is
unavailable. A closed or unavailable posting remains recordable when Cast retains
the job. Clew does not assert which packet the user used.

The positional `clew record REFERENCE` form accepts only a retained legacy Platter
alias from migration. Use `--cast-job` for a Cast job ID. These are distinct
argument namespaces; Clew does not guess from the text of an opaque identifier.

Translate an explicit statement such as "I applied" into the supplied status
`applied`. Never assign status from posting availability, Platter delivery,
correspondence inspection, elapsed time or notes. An unclear report requires
clarification before recording a status. These commands do not apply, send,
contact an employer, edit Cast or Platter, or start a model or schedule.

Choose a stable write ID before invocation. IDs contain 1 through 128 ASCII
letters, digits, dots, underscores or hyphens. Reuse the same ID and identical
arguments after an uncertain result. The retry returns the original entry,
including after that entry is superseded. Reusing an ID with different content
fails. Migration preserves the original argument namespace. Retry a legacy write
with its original positional reference; substituting `--cast-job` is different
content even when the alias identifies that job. The write commits before its
success response.

## Read current status and history

```sh
clew list
clew show REFERENCE
```

Show accepts a Cast job ID or a retained legacy Platter alias. An identifier that
could select different jobs fails as ambiguous. Its result contains
`cast_job_id`, `current` and `history`.

List returns each currently tracked job, its latest supplied status,
the supplying `status_entry_id`, and its latest active record. Current status is
the last nonnull status by ledger sequence among records that have not been
replaced or retracted. If no such status exists, it is null. If all records for
a job are retracted, it is absent from list but remains readable in show.

Show returns the current record and full history for that exact job,
including replacements that moved a report to another job. Each history
entry identifies its immediate `superseded_by` entry when present. The complete
original text remains retained. The current read and history read are successive
local observations; another writer can commit between them.

An entry contains a sequence, write ID, `recorded_at`, kind, `cast_job_id`,
optional `platter_job_ref`, optional status, optional notes and optional `replaces`.
The Platter reference preserves the exact legacy argument when present.
Sequence orders committed
appends. `recorded_at` is the UTC RFC3339 time Clew recorded the report. It is not
the date of application or response. Put user-supplied historical dates in notes.

All commands return JSON with `ok`, `schema_version: 2`, and `data`, or a nonzero
exit with an error detail. `--json` is accepted for explicit callers. Reads return
complete selected histories, with no paging or automatic pruning.

## Correct a report

```sh
clew record --cast-job CORRECT_JOB_ID --id CORRECTION_ID --replaces ENTRY_ID \
  --status applied --notes 'Corrected report.'
clew retract ENTRY_ID --id RETRACTION_ID --notes 'This report concerned another job.'
```

A replacement supplies a complete new latest report. Omitted fields become null;
they are not copied from the target. A supplied status can therefore become the
current status. For a notes-only correction, supply only the corrected notes.
Retraction removes the target from current reads and can reveal an earlier
supplied status. Its explanatory notes are retained as correction history.

Only an active record can be corrected or retracted. A retraction cannot itself
be retracted. To restore a report, append it under a new ID. Correct an already
replaced report by selecting its active replacement. Wrong-job corrections may
select another exact job, with the ordinary first-job check.

Each operation appends one transaction. SQL triggers prohibit updates and deletes
of ledger rows. Invalid targets and conflicting write IDs leave history unchanged.

## Storage and failures

The default database is `~/.local/share/clew/ledger.sqlite3`, a private regular
file with mode 0600 in a directory with mode 0700. `--state-dir ABSOLUTE_PATH`
selects an explicit independent private ledger. Ordinary operations require
initialized schema-two state and never initialize or migrate it implicitly.
Use the [installation contract](install-operate.md) for initialization and recovery.

SQLite serializes short writes and waits up to five seconds for contention.
Read operations use memory proportional to retained history. No hard size,
latency, future compatibility or external availability guarantee is supplied.
An interrupted append may have committed; recover with its unchanged write ID.

Keep notes, statuses and job interests private. Commands print them only to the
caller. Clew stores no credentials and makes no network requests. Agent dispatch
attempts metadata-only Chancery usage recording; errors preserve the operation.
Dependency reads are marked internal and excluded from recording.

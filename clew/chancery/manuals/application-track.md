# Record application status and notes

Clew stores what the user reports about a prepared Platter opportunity. The
user supplies every status. The conversational agent finds the intended job
and resolves material ambiguity before writing. Clew accepts an exact reference;
it does not interpret prose or choose a candidate.

## Find the opportunity

```sh
clew find 'company, role, nickname or URL'
```

Find reads all retained Platter opportunities through `platter.opportunity.explore`
and searches references and Clew notes. Notes from superseded records are included
as search clues. Platter matches companies, roles, IDs and URLs. HTTP URL queries
use Platter's supported ATS identity or URL normalization. These local reads make
no network request. The two products are read in separate snapshots.

The result contains all `candidates`, any matching
`retained_references_without_platter_record`, and `complete: true`. A missing
Platter reader or failed read is an error, not a complete empty result. Use
`list` or `show` for retained Clew history while Platter is unavailable.

Use the conversation and returned company, role, URLs and packet metadata to
identify the intended opportunity. Ask the user when plausible choices remain.
An unknown nickname requires clarification. A URL with no prepared match does
not authorize collection or preparation.

## Append the supplied report

```sh
clew record REFERENCE --id WRITE_ID --status applied --notes 'Applied today.'
clew record REFERENCE --id NEXT_WRITE_ID --notes 'Recruiter asked about availability.'
```

Supply at least one nonblank `--status` or `--notes`. Status is free text, with
no required sequence of transitions. Notes retain the supplied text. Omit a
field to store null. A notes-only entry leaves current status unchanged.

The first record for a reference must resolve through the installed Platter
reader. An already recorded exact reference can receive further reports while
Platter is unavailable. The reference identifies the opportunity across resume
regenerations; Clew does not assert which packet the user used.

Translate an explicit statement such as "I applied" into the supplied status
`applied`. Never assign status from posting availability, Platter delivery,
correspondence inspection, elapsed time or notes. An unclear report requires
clarification before recording a status. These commands do not apply, send,
contact an employer, edit Platter or start a model or schedule.

Choose a stable write ID before invocation. IDs contain 1 through 128 ASCII
letters, digits, dots, underscores or hyphens. Reuse the same ID and identical
arguments after an uncertain result. The retry returns the original entry,
including after that entry is superseded. Reusing an ID with different content
fails. The write commits before its success response.

## Read current status and history

```sh
clew list
clew show REFERENCE
```

List returns each currently tracked opportunity, its latest supplied status,
the supplying `status_entry_id`, and its latest active record. Current status is
the last nonnull status by ledger sequence among records that have not been
replaced or retracted. If no such status exists, it is null. If all records for
a reference are retracted, it is absent from list but remains readable in show.

Show returns the current record and full history for that exact reference,
including replacements that moved a report to another opportunity. Each history
entry identifies its immediate `superseded_by` entry when present. The complete
original text remains retained. The current read and history read are successive
local observations; another writer can commit between them.

An entry contains a sequence, write ID, `recorded_at`, kind, `platter_job_ref`,
optional status, optional notes and optional `replaces`. Sequence orders committed
appends. `recorded_at` is the UTC RFC3339 time Clew recorded the report. It is not
the date of application or response. Put user-supplied historical dates in notes.

All commands return JSON with `ok`, `schema_version: 1`, and `data`, or a nonzero
exit with an error detail. `--json` is accepted for explicit callers. Reads return
complete selected histories, with no paging or automatic pruning.

## Correct a report

```sh
clew record CORRECT_REFERENCE --id CORRECTION_ID --replaces ENTRY_ID \
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
select another exact reference, with the ordinary first-reference check.

Each operation appends one transaction. SQL triggers prohibit updates and deletes
of ledger rows. Invalid targets and conflicting write IDs leave history unchanged.

## Storage and failures

The default database is `~/.local/share/clew/ledger.sqlite3`, a private regular
file with mode 0600 in a directory with mode 0700. `--state-dir ABSOLUTE_PATH`
selects an explicit independent private ledger. Ordinary operations require
initialized schema-one state and never initialize or migrate it implicitly.
Use the [installation contract](install-operate.md) for initialization and recovery.

SQLite serializes short writes and waits up to five seconds for contention.
Read operations use memory proportional to retained history. No hard size,
latency, future compatibility or external availability guarantee is supplied.
An interrupted append may have committed; recover with its unchanged write ID.

Keep notes, statuses and job interests private. Commands print them only to the
caller. Clew stores no credentials and makes no network requests. Agent dispatch
attempts metadata-only Chancery usage recording; errors preserve the operation.
Dependency reads are marked internal. `clew --register-usage` registers commands
separately after installation without adding ledger entries.

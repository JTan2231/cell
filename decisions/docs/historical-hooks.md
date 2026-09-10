# Prepare historical hooks

Use `scripts/replay_hooks.py` to prepare Stop-hook inputs from retained local
Codex history. Python 3.9 or later is required. Generation reads Conversations
and writes private local files. Only the explicit `replay` command calls Krisis.

## Generate a preview

Select the same Codex executable as the installed Krisis observer. Its path is
`CONVERSATIONS_CODEX` in the selected `krisis/observer` Clockwork definition.

```sh
python3 decisions/scripts/replay_hooks.py generate \
  --days 30 --timezone America/Chicago \
  --codex /absolute/path/to/codex \
  --output /absolute/path/to/new-preview-directory
```

The window ends at the start of generation. Use `--until UNIX_SECOND` to select
a fixed exclusive endpoint. The start is exactly `--days * 86400` seconds
earlier. Daily counts use `--timezone`, which defaults to UTC.

The script checks Conversations readiness, lists all active and archived
interactive root tasks updated since the window start, then reads each task
through `conversations show`. It increases the listing limit until `has_more`
is false. Four readers run concurrently; `--workers N` changes that count.
It selects turns by completion time, including the start and excluding the end.
Copied message IDs belong to the first task in Conversations' newest-first
listing, matching its export deduplication. A selected turn must retain a
nonblank user message. No decision classification occurs during generation.

The new output directory contains:

- `hooks.jsonl`: one exact Stop-hook input per selected turn, sorted by completion
  time, task ID, and turn ID. The canonical task ID supplies `session_id`.
- `turns.json`: the corresponding task titles, working directories, archive
  flags, completion times, and hook inputs. It contains no message bodies.
- `summary.json`: the exact window, source identity, hook-file digest, counts
  by day and task, exclusions, and coverage gaps. `ingested: 0` describes generation.

Unreadable tasks are reported individually while other tasks are inspected.
Source errors, unknown turn status, or missing completion time make coverage
incomplete and cause a nonzero exit. Exclusion counts describe all histories
read; timing gaps cannot be assigned reliably to the selected window. Preview
files from an incomplete generation require source repair or explicit exclusions
for every unreadable task before replay. Unknown status and completion-time gaps
always block replay. The script does not refresh Codex metadata
or read Codex or Krisis databases directly.

## Replay a reviewed preview

```sh
python3 decisions/scripts/replay_hooks.py replay \
  --input /absolute/path/to/preview-directory
```

To omit reviewed unreadable tasks, add `--exclude-unreadable-task THREAD_ID`
once for each task reported with `history_read_failed` in `summary.json`.
Every such gap must be named, and no other gaps or generation errors may remain.
The original preview retains its incomplete coverage report. Replay refuses
any hook whose task is excluded and reports the exclusions with its admission count.

Replay validates the whole file and its digest before the first admission. It
sends each object to `krisis observe ingest` through stdin and stops on the first
error. `--krisis` selects an explicit executable. Repeating the same preview
reuses its correlations; Krisis deduplicates existing observations. Failed or
previously ineligible observations require separate recovery and are not reset.

The replay count describes successful ingest calls, including duplicates. It
does not count new observations, classifications, decisions, or Annals receipts.
Krisis owns those outcomes. Its configured observer processes the queue serially.
Submission order does not guarantee chronological classification order.

The observer's current baseline still applies. Each classification reads the
full normalized history through its selected turn, including context before the
preview window. Existing source and prompt-size limits still apply. See
[source documents](source-documents.md) and [the observer commands](cli.md).

# Archive or unarchive one want

Use this capability only when the user directly requests the transition for
one identified captured want. Do not infer archival from completion, inactivity
or decision associations. Scheduled processing never changes lifecycle state.

```sh
/Users/joey/.local/bin/conatus want archive WANT_ID
/Users/joey/.local/bin/conatus want unarchive WANT_ID
```

Conatus state must be initialized and writable. Global `--state-dir ABS_PATH`
selects private state; the default is `CONATUS_STATE_DIR` or
`~/Library/Application Support/Conatus`. Product maintenance holds block both
commands. Annals need not be available.

New and existing wants default to `active`. Archive makes the want inactive;
unarchive restores the same want to active. Archived does not mean completed.
Both commands preserve its ID, wording, source reference, capture time and
frozen outgoing document. An archived want remains readable. Unarchive is its
only lifecycle change. No source-edit or deletion operation is added.

The response is JSON `{ "ok": true, "data": { "changed": true, "record": ... } }`.
The record includes `state`, either `active` or `archived`. Success means the
local transaction committed. Repeating the same command succeeds with
`changed:false`. An unknown ID, a decision ID or a storage error returns
`{ "ok": false, "error": "..." }`. Inspect `want show WANT_ID` after an
interrupted command. Repeating the requested transition is safe.

The existing schema-one settings table stores only a current archive marker.
Absence means active. Unarchive removes the marker. No migration, archival
reason, timestamp, actor or transition history is required. Older releases
ignore these markers and can include archived wants in lists and new emails.
Use a release with lifecycle support when archive filtering is required.

`want list` selects active wants by default. `want list --archived` selects
archived wants; `want list --all` selects both states. `want show WANT_ID`
reads either state. Status reports `local.wants.active` and
`local.wants.archived`; intake and handoff counts still include both states.

Newly rendered emails include only active wants. Retained email occurrences
keep their original bytes for preview and explicit retry. Archiving after an
email was frozen does not change that occurrence or recall submitted mail.

State stays local to Conatus. The commands do not invoke Annals, a model,
Email or Clockwork. Pending handoffs, processing receipts, retained sources,
re-examination and Annals associations continue independently. No state is
propagated to related wants, decisions, outgoing documents or Annals graphs.

This operation authorizes only the requested local transition. It does not
authorize publication, sending email, source edits, deletion, or unrelated
product changes. No wall-clock completion or cross-release compatibility
window is promised.

## Command usage

CLI dispatch separately attempts to record system and command identity,
observation time and optional thread attribution in Chancery's private usage
journal. It stores no arguments or output and does not establish transition
history. Recording errors preserve product results. The separate
`--register-usage` mode registers the command inventory without product work.

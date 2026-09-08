# CLI and operation

All commands return JSON `{ "ok": true, "data": ... }` or
`{ "ok": false, "error": "..." }`. `--json` is accepted but not required. Use the global
`--state-dir PATH` to select Conatus state explicitly. Paths in examples are
operator selections, not a request to initialize or activate a deployment.
The path must be absolute. Without it, Conatus uses `CONATUS_STATE_DIR` or
`~/Library/Application Support/Conatus`.

## Initialize

```sh
conatus --state-dir STATE init --annals ANNALS_BIN --decisions-config DECISIONS_CONFIG --library conatus
```

`--annals-state-dir PATH` selects the Annals named-library catalog when needed.
The library name defaults to `conatus`. Initialization creates or selects a
named general library, selects [the bundled instructions](../librarian.md), pins its persistent
identity and that of the decisions library, and stores the current feed
watermark. It starts no model and installs no timer. The decisions configuration
must explicitly select the existing dedicated decisions library.

## Capture and read

```sh
conatus want add 'I want more time to write.' --source 'conversation reference'
conatus want add --file want.txt --source 'conversation reference'
conatus want add --stdin --source 'conversation reference'
conatus want list --limit 20
conatus want show ID
conatus decision list --limit 20
conatus decision show ID
conatus graph
conatus history --limit 20
conatus status
```

Supply exactly one nonblank UTF-8 want through the argument, file, or standard
input. Conatus preserves its wording and the supplied source-reference string.
It adds no inferred conditions, fields, or further wants. Capture records local
intake; `update` forwards pending inputs for Annals retention and interpretation.
The caller must select an actual source statement expressing the user's want,
rather than an assistant suggestion or a hypothetical example.

Want and decision lists select Conatus intake records by source kind. `show`
selects one intake identity and reports its retained source and available Annals
evidence. The graph and history read the selected Annals library. A concept's
label need not match the full source statement. Source wording remains the
authority for what was captured.

Intake lists default to 20 records and accept limits from 1 through 100. Want IDs
use `want-` plus UUIDv7. Decision IDs use `decision-` plus SHA-256 of source
library ID, colon, and event ID. A record contains `id`, `kind`, `source`,
`wording`, `source_data`, `work_name`, `captured_at`, `queued_at`, `receipt`, and
`error`. The work name is the intake ID; the outgoing filename adds `.md`.
`wording` is exact want text or the supplied decision statement; `source_data` preserves the
source-reference object or original typed feed event. A nullable handoff time
or receipt describes enqueue, not successful model integration.

`show` keeps `record`, `retention`, `interpretation`, and `associations` separate.
Each Annals read reports its availability and data or error. `related_records`
maps returned associations back to captured wants and decisions, with their
exact wording, source reference, direction, and direct or path relationship.
It includes only Conatus intake records grounded in the returned graph; the
association view's completeness flags also apply to this projection. Status remains
usable when Annals is unavailable and keeps the Annals inbox read separate from
local intake counts, the cursor, pause, and last update report. A partially
failed update retains that report before returning an error.

Intake timestamps are UTC Unix seconds recording Conatus capture. Decision
occurrence times retain the feed's separate precision. Annals revision and
delivery times describe those Annals operations. The feed cursor describes
intake coverage; it does not describe graph freshness.

## Update and recover

```sh
conatus update
conatus pause
conatus resume
conatus retry --from ANNALS_JOB_ID --through ANNALS_JOB_ID
conatus reexamine ID
```

An update consumes all new accepted decision events through its chosen
watermark, forwards pending frozen documents, and runs the Annals inbox. Inbox
integration can use the configured model allowance and automatically applies
valid material results. Only one Conatus update mutates the selected state at a
time.

`pause` gates subsequent `update` calls. An active update finishes, and explicit
retry or re-examination remains available. `resume` releases that product pause;
it does not install or enable a schedule. These operations do not cancel an
already admitted Annals job.

Use status and the selected record to distinguish a capture or handoff error
from an Annals processing failure. Captured input survives a failed handoff.
Queued source bytes survive model unavailability. `retry` explicitly selects a
bounded Annals job interval; its boundaries are job IDs, not Conatus intake IDs.
Annals retry start requires its inbox paused with no active processing job. Use
the selected named-library `inbox pause` first; Conatus' local pause does not
set the Annals pause. Follow a halted event through Annals `inbox retry status`
and `inbox retry continue`, then resume the Annals inbox when ready.
`reexamine` requests fresh interpretation of one retained source and applies a
valid material result. It does not retain an input that has not reached Annals.
Neither a duplicate source delivery nor the absence of a
new corpus revision proves that a model examination succeeded.

## Library instructions

```sh
conatus instructions show
conatus instructions set --file instructions.md
```

The exact nonblank UTF-8 document becomes the library's selected instructions.
Selection starts no model and rewrites no history. Re-examine selected retained
records explicitly when existing organization should reflect new instructions.

Annals owns instruction revisions, immutable source bytes, evidence, corpus
revisions, and reconciliation success. Use its supported named-library commands
for deeper inspection. Do not edit its SQLite database or spool directly.

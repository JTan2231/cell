# CLI and operation

Commands return JSON `{ "ok": true, "data": ... }` or
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
named general library, selects the Bazaar `conatus.library.instructions` document, pins its persistent
identity and that of the decisions library, and stores the current feed
watermark. It starts no model and installs no timer. The decisions configuration
must explicitly select the existing dedicated decisions library.

To select a replacement Annals executable, repeat `init` with the same library,
decisions config, and Annals state root. Conatus verifies both pinned library
identities before saving the new executable path. It preserves the cursor,
records, instructions, and pause state. Update any Clockwork definition
separately after selecting a new Conatus release.

## Capture and read

```sh
conatus want add 'I want more time to write.' --source 'conversation reference'
conatus want add --file want.txt --source 'conversation reference'
conatus want add --stdin --source 'conversation reference'
conatus want list --limit 20
conatus want list --archived
conatus want list --all
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

Want lists select active wants by default. `--archived` selects archived wants;
`--all` selects both states. These options conflict. State selection precedes
the limit and `has_more` calculation. Decision lists select all decision intake.
`show`
selects one intake identity and reports its retained source and available Annals
evidence. The graph and history read the selected Annals library. A concept's
label need not match the full source statement. Source wording remains the
authority for what was captured.

Intake lists default to 20 records and accept limits from 1 through 100. Want IDs
use `want-` plus UUIDv7. Decision IDs use `decision-` plus SHA-256 of source
library ID, colon, and event ID. A record contains `id`, `kind`, `source`,
`wording`, `source_data`, `work_name`, `captured_at`, `queued_at`, `receipt`, and
`error`. The work name is the intake ID; the outgoing filename adds `.md`.
Want records also contain `state`, either `active` or `archived`. Decision
records have no lifecycle state. Status reports `local.wants.active` and
`local.wants.archived`; existing intake and handoff counts include both states.
`wording` is exact want text or the complete supplied decision document; `source_data` preserves the
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

Graph and association views return at most 200 concepts. Each concept preview
includes at most 20 parents, 20 children, and 20 evidence entries. Check
`concepts_complete`, `relationships_complete`, and `evidence_complete` before
treating the returned view as complete for its Annals revision.

Intake timestamps are UTC Unix seconds recording Conatus capture. The feed's
`accepted_at` records Annals acceptance, not when a decision occurred. Annals revision and
delivery times describe those Annals operations. The feed cursor describes
intake coverage; it does not describe graph freshness.

## Archive and unarchive

```sh
conatus want archive WANT_ID
conatus want unarchive WANT_ID
```

Use either command only after the user directly requests that transition for
the selected want. New and existing wants default to `active`. Archive makes a
want inactive; unarchive restores the same want to active. Neither state asserts
completion. A repeated command succeeds with `changed:false`. An unknown ID or
a decision ID fails. The result contains `changed` and the saved `record`.

Both commands change only local current state and work without Annals. They
preserve identity, wording, source, capture time and outgoing document bytes.
An archived want remains readable. Its only lifecycle change is unarchive.
Conatus stores no archival reasons, timestamps, actors or transition history.
Product maintenance holds block both commands.

Archive state stays in Conatus. Pending handoffs, receipts, interpretation,
re-examination and Annals associations continue independently. Archival never
rewrites or removes retained sources or propagates to related wants or decisions.
The default want list and newly rendered emails exclude archived wants. A frozen
email occurrence keeps its original bytes for retained preview and explicit retry.

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

An update ends on its first feed, handoff, or inbox failure. It preserves its
completed stage report and starts no successor stage. Its Annals batch selects
`--stop-on-failure`. When invoked by the configured Clockwork schedule, a failed
update halts future scheduling until `clockwork binding resume conatus/update
INCIDENT_ID` is explicitly approved. `conatus resume` only clears the product's
operator pause; it does not clear a scheduling incident.

## Daily email

Use `conatus email preview` to read the complete plain-text message, or add
`--json` for structured output. `conatus email send` sends an ad hoc occurrence.
See [daily email](email.md) for exact content, daily scheduling, retained
occurrences, submission recovery and privacy. These commands start no model.

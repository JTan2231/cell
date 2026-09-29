# Intake, interpretation and recovery

Use this feature to understand initialization, new decision intake, Annals
handoff, instruction selection and explicit recovery. Use
`conatus.update.operate` for the operating procedure. Capture, want archive state,
read views and email have their own feature contracts.

## Interfaces and prerequisites

```sh
conatus --state-dir ABS_STATE init --annals ABS_ANNALS --decisions-config ABS_CONFIG --library conatus
conatus update
conatus pause
conatus resume
conatus retry --from ANNALS_JOB_ID --through ANNALS_JOB_ID
conatus reexamine INTAKE_ID
conatus instructions show
conatus instructions set --file instructions.md
```

Use global `--state-dir ABS_PATH` for private Conatus state. The default is
`CONATUS_STATE_DIR` or `~/Library/Application Support/Conatus`. Product commands
return JSON `{ "ok": true, "data": ... }` or
`{ "ok": false, "error": "..." }`; `--json` is optional.

Initialization needs writable Conatus state, the supported Annals executable,
a named general library and an explicit existing decisions-library config.
The library name defaults to `conatus`.
`--annals-state-dir ABS_PATH` can select the Annals catalog. Processing requires
the pinned identities and available Annals inbox. Interpretation uses Annals'
configured authenticated Nucleus path and can consume account allowance.
Conatus has no separate Nucleus toolset or authentication authority.

## Initialization and identity

First initialization creates or selects the named general library, selects the
exact `conatus.library.instructions` document through Bazaar's
`cell.prompts.conatus` selection, pins the general and decisions-library
identities, and saves the current accepted-feed watermark as the baseline.
It starts no model and enables no schedule. It imports no earlier decisions.
There is no historical-import operation or implicit decisions-library selection.
Existing Annals instruction revisions remain unchanged.

Repeated `init` can replace the Annals executable only after both pinned library
identities pass verification. The same library, decisions config and Annals
state root must be selected. It preserves the cursor, records, instructions and
pause state. Its result reports `initialized:false` and whether the executable
was `rebound`. Program rebinding does not retarget an immutable Clockwork
schedule; `conatus.service` owns that boundary.

## Complete documents and handoff

An update chooses an accepted-feed watermark and consumes all new events through
that point. Each event and its opaque cursor advancement commit in the same
local transaction. The cursor is bound to the source-library identity.

Decision intake saves the original event, complete unchanged UTF-8 document,
source filename, digest, acceptance time and transport identities. It forwards
exactly that text. It requires no account fields or source lookup and does not
reconstruct an account. Existing structured decision projections remain
historical sources. Acceptance does not prove enactment, progress or current
force. Annals exchange contract 2 owns accepted-document semantics.

Feed pages can stop at 4 MiB of document bytes before the requested count.
Update continues after every nonempty page until an empty page completes the
selected prefix. The cursor measures intake coverage, including sources whose
interpretation is pending or failed.

New documents are enqueued before retention. Annals retains them and interprets
them under the selected library instructions, automatically applying valid
material results. A fresh inbox duplicate whose bytes already identify a
retained work does not request another examination. Explicit re-examination
uses Annals integration instead. Conatus serializes its update paths. Use its
runner as the only scheduled driver of the dedicated inbox.

Captured means durable local intake. Queued means an accepted Annals handoff.
Retained means an Annals work exists. Interpreted means Annals recorded a domain
interpretation result. A no-change interpretation need not advance the corpus
revision. Enqueue and process exit do not prove interpretation.

Conatus and Annals do not share one transaction. Generic enqueue has no
producer-key idempotency. An uncertain handoff can create a duplicate delivery;
work byte identity alone does not prove an examination. Intake times are UTC
Unix seconds. `captured_at` describes local capture and `queued_at` successful
handoff. Feed `accepted_at` describes Annals acceptance. Annals delivery and
revision times describe those operations, not decision occurrence.

## Instructions and associations

Instruction replacement accepts exact nonblank UTF-8 bytes. It appends the
library's selected instruction revision, starts no model, rewrites no source
and does not reinterpret history. Annals freezes corpus and instruction
revisions at examination admission and rejects stale material application.
A recorded or committed domain result survives a later runtime failure.

The instructions permit source-grounded associations in which a child appears
to serve a parent want. Multiple parents and unassociated decisions are allowed.
They do not permit invented wants or qualifications. A model-created concept
is not another captured want. A direct edge or path does not establish measured
progress, enactment, completion or current force. Conatus does not run
transitive reduction automatically.

## Pause, failures and recovery

Pause gates subsequent updates. An active update finishes; explicit retry and
re-examination remain available. Resume releases only this local gate. Neither
command changes a Clockwork binding, sets the Annals pause or cancels an admitted
Annals job. Archive state never gates these processing paths.

An update stops on its first feed, handoff or inbox failure, preserves completed
results and its partial report, and starts no successor stage. Annals dispatch
uses `inbox run --stop-on-failure`. Status keeps the report available independently
of Annals readiness. Empty work and Annals low-storage dispatch readiness are
successful outcomes. Failed enqueue, including insufficient copy capacity,
is a failure. `conatus.service` owns the scheduled halt boundary.

Captured input survives handoff failure. Normal update resumes pending intake
and queued work when dependencies return. Failed Annals attempts require an
explicit bounded retry. Retry selects failed Annals job IDs in inclusive failed
delivery-completion order, not Conatus intake IDs. Retry start requires the
Annals inbox paused with no active processing job; Conatus pause is separate.
Annals owns retry-event eligibility, progress and halts. Inspect a halted event
through its supported retry status and continue interfaces before continuing.

Re-examination selects one retained input for fresh model interpretation and
valid material application. It does not retain an input that has not reached
Annals. Inspect domain receipts before treating runtime failure as failed
interpretation. Do not reset the baseline, create open-ended retry, or re-enqueue
retained bytes to request another examination. Use the selected named-library
Annals commands for domain recovery; do not edit its database or spool.

## Bazaar selection

Library initialization needs initialized private Bazaar state and a complete
`cell.prompts.conatus` selection. The default database is
`~/.local/share/bazaar/bazaar.sqlite3`; an absolute `CELL_BAZAAR_DATABASE` override
selects another database. Reads fail without creating state or using fallback
text. Deployment does not import missing prompt contents.

The selection content is
`{"schema_version":1,"entries":{"PROMPT_ID":VERSION}}`.
Every component is pinned to a positive integer version. Publish component text
before the complete selection. A text append alone does not change selection.
Missing or invalid selection stops new preparation before model admission.
Preserve migration selection version 1 and its referenced text versions.

Resolved instructions become an immutable library revision under the existing
Annals operation. Retained work and retries keep their existing selected domain
snapshots. Later prompt edits do not rewrite saved work. Models, permissions,
structural schemas, domain commits and recovery remain product-owned.
Use `conatus.update.operate` for the prompt-edit procedure.

## Authority, privacy and limits

Conatus owns intake, cursor, outgoing documents, receipts, local processing
gates and the selected instruction document. Annals owns library and instruction
identities, immutable works, graph, evidence, revisions, interpretation,
model integration and domain recovery. Nucleus owns execution.

Full private statements, documents, references and evidence can remain in
Conatus state, Annals works and spool, and Nucleus context. Integration can
send source and relevant corpus context through Annals to its configured model.
No publication, unrelated lifecycle action, credential operation, direct
database edit or user-data cleanup is authorized by this feature.

Conatus state remains schema 1. Annals schemas, exchange contract, Clockwork
definitions, provider release and feature versions are separate. No general
cross-release compatibility window, storage capacity, activation delay,
queue-drain deadline or interpretation completion time is promised.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time and thread ID, not arguments, output or
outcomes. Internal calls are excluded. Recording errors preserve product results.

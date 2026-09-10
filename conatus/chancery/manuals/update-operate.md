# Initialize and operate Conatus

Conatus owns want and decision intake, its feed cursor, frozen outgoing
documents, handoff receipts, and the interpretation document. Annals owns the
dedicated library, immutable works, evidence, graph, model integration, and
domain recovery. Conatus uses Annals as its model requester; it has no separate
Nucleus toolset or authentication authority.

All product commands return JSON success data or error text. `--json` is
optional; global `--state-dir ABS_PATH` selects Conatus state. The default is
`CONATUS_STATE_DIR` or `~/Library/Application Support/Conatus`.

## Initialize and process

```sh
/Users/joey/.local/bin/conatus --state-dir STATE init --annals ANNALS_BIN --decisions-config DECISIONS_CONFIG --library conatus
/Users/joey/.local/bin/conatus update
/Users/joey/.local/bin/conatus status
```

Initialization optionally accepts `--annals-state-dir PATH`. It creates or
selects a named general library, selects the bundled exact instruction document,
pins its persistent identity and the explicitly configured decisions-library
identity, and stores the current accepted-account watermark as its baseline.
The bundled document is `conatus/librarian.md` in the product source.
It starts no model and enables no schedule. There is no historical-import
operation or implicit decisions-library selection.

To replace the pinned Annals executable, repeat `init` with the same library,
decisions config, and Annals state root. Conatus verifies both pinned library
identities before saving the new executable path. The cursor, records,
instructions, and pause state remain unchanged. The result reports
`initialized: false` and whether the executable was `rebound`. Cell deployment updates its selected Clockwork definition during configuration.

An update consumes all new accepted events through its chosen watermark. Each
event and cursor advancement commit in the same local transaction. The feed
provides complete accepted document text, source filename, digest, acceptance
time, and transport identities. Conatus forwards the exact text and lets the
configured library agent interpret it.

New sources are enqueued before retention. Annals retains them and integrates
them with automatic application of valid material changes. A fresh duplicate
whose bytes already identify a retained work does not request an examination.
Conatus serializes its own update paths; use this runner as the only scheduled
driver of its Annals inbox.

Captured, queued, retained, and interpreted identify separate successful
operations. Intake counts select local wants or decisions; pending/queued counts
describe handoffs. The stored feed cursor marks intake coverage, not successful
interpretation. Intake times use UTC Unix seconds; source occurrence precision
and Annals revision times remain separate. Generic enqueue has no producer-key
idempotency or cross-library transaction. An uncertain handoff can create a
duplicate delivery; work byte identity alone does not prove interpretation.

## Recover or change the interpretation

```sh
/Users/joey/.local/bin/conatus pause
/Users/joey/.local/bin/conatus resume
/Users/joey/.local/bin/conatus retry --from ANNALS_JOB_ID --through ANNALS_JOB_ID
/Users/joey/.local/bin/conatus reexamine INTAKE_ID
/Users/joey/.local/bin/conatus instructions show
/Users/joey/.local/bin/conatus instructions set --file instructions.md
```

Pause gates subsequent update calls; an active update finishes, and explicit
retry or re-examination remains available. Resume releases that gate. Neither
changes a Clockwork binding or cancels an admitted Annals job. Local input
survives handoff failure. Normal update resumes pending intake and queued work
when dependencies are available. Failed Annals attempts require an explicit
bounded retry. Retry boundaries are failed Annals job IDs in inclusive failed
delivery-completion order, not Conatus IDs. Re-examination selects one retained
input for fresh model interpretation and valid material application.

Retry start requires the Annals inbox paused with no active processing job.
Conatus' local pause does not set the Annals pause. Use the configured Annals
executable and catalog selection with `annals library NAME inbox pause` before
starting the interval. Resume that Annals inbox after recovery when ordinary
dispatch should continue. Re-examination does not implicitly retain an input
that has not reached the Annals library.

Annals owns retry-event eligibility and progress. If an event halts, inspect its
receipt and use its supported retry status/continue interface for that event;
do not create an open-ended retry or re-enqueue retained bytes to request a new
examination. Use the selected named-library Annals commands for deeper domain
recovery. Do not edit its database, job files, or spool directly.

Instruction replacement selects exact nonblank UTF-8 bytes. It starts no model,
does not rewrite sources, and does not reinterpret history. Annals freezes
corpus and instruction revisions at examination admission and rejects stale
material application. A committed or recorded result survives a later runtime
failure. Inspect domain receipts before treating an execution failure as a
failed interpretation. Conatus does not run transitive reduction automatically.

The instructions permit service-to-want associations grounded in captured
sources. They do not permit invented wants or qualifications. Such associations
do not establish progress, enactment, completion or current force.

## Install and separately activate

```sh
conatus-install install --binary ABS_BINARY --bundle ABS_BUNDLE --expected-current absent
conatus-install inspect
conatus-install verify --binary ABS_BINARY --bundle ABS_BUNDLE
conatus-install verify-release ABS_RELEASE
conatus-install recover --release ABS_RELEASE --expected-current releases/HASH
```

The default install root is `~/Library/Application Support/Conatus/install`.
Packaging commands accept `--home ABS_HOME`. Install stages the release and its
matching Chancery bundle and selects the candidate. Upgrade uses the observed
`releases/HASH` instead of `absent`. Recovery selects an exact retained release;
it does not revert domain databases or Clockwork bindings. Installation starts
no model, initializes no runtime state, and changes no active schedule.

Prepare a new immutable definition from the selected installed release after
the Conatus state directory exists:

```sh
conatus-install schedule-definition --state-dir ABS_STATE --output ABS_DEFINITION
clockwork definition register ABS_DEFINITION
clockwork binding switch conatus/update DEFINITION_DIGEST
clockwork binding show conatus/update
clockwork history conatus/update --limit 20
clockwork binding disable conatus/update
```

Definition generation requires a new output file under an existing parent. It
writes that file and creates the private state log directory, but neither
registers nor activates the runner. The definition pins the current installed
release, runs `update` every 300 seconds with run-at-load enabled and overlap
skip, and writes product output under `STATE/logs/`. It contains no credential.
Clockwork registration returns the digest; binding selection activates it.

Before switching, establish the product's ready initialized state and retain
the prior binding/release selection for recovery. Direct program installation does not retarget an existing immutable definition.
Cell deployment performs this retargeting during configuration. Do not keep old and new inbox schedules active together.

These operations require available local Annals interfaces; integration also
requires Annals' configured authenticated Nucleus execution path. Clockwork
supplies activation rather than domain success. Installation, registration and
an exit-zero process result do not prove retention or interpretation. No
maximum activation delay, queue-drain deadline, interpretation time, storage
capacity, or cross-release compatibility window is promised.

Conatus state, Annals spools and works, and Nucleus context can retain complete
private wording, full documents and evidence. Model integration may consume the
configured account allowance. No publication, cleanup of user data, direct
database mutation, unrelated lifecycle action, or credential operation is
authorized by these commands.

Decision intake uses Annals exchange contract 2. It saves the original event,
complete document, source filename, digest, acceptance time, and transport
identities with its cursor, then forwards exactly that text to the Conatus
library. No account fields or source lookup are required. Library instructions
govern connections and interpretation. Feed pages may stop at 4 MiB of document
bytes before reaching the requested count; update continues until an empty page.

## Scheduled failure policy

Conatus generates Clockwork definition schema 2 for `conatus/update`, with
`[failure] on_abend = "halt-until-approved"`. An update stops admitting work on
its first feed or handoff error. Annals dispatch uses `inbox run
--stop-on-failure`, so a failed source also stops its batch. The update retains
completed stage results before returning nonzero. A failed feed no longer starts
pending handoffs, and a failed handoff no longer starts inbox processing.

Clockwork owns the durable scheduling halt and one retained email notification
through `HOME/.local/bin/email`. Inspect `clockwork incident list conatus/update`
and `clockwork incident show INCIDENT_ID`. After explicit approval, use
`clockwork binding resume conatus/update INCIDENT_ID` to allow future scheduling.
A new definition, deployment, `conatus resume`, or Annals recovery cannot clear
that incident. Schema-one bindings acquire this behavior only after an explicit
schema-two definition switch; definition generation does not activate it.

Conatus' operator pause, Annals' operator pause and bounded retry-event halts,
source identities, handoff receipts, and explicit domain retry remain with their
products. Empty work and Annals low-storage dispatch readiness are successful
outcomes. A failed enqueue, including insufficient copy capacity, is an abend.
Continuation permits pending handoffs under their existing identity rules; it
does not give a failed Annals attempt another try.

## Coordinated deployment setup

Cell deployment captures Conatus configuration, product pause and the exact
`conatus/update` selection. It holds durable admission, temporarily pauses
updates and disables the selected binding. Drain waits for admitted commands
and the prior runner lock. Installation selects programs first; configuration
then initializes absent state or rebinds the final Annals executable through
`init`. Existing library identities, cursor, records and instructions remain.

Deployment settings accept `state_dir`, `decisions_config`, `annals_state_dir`,
`library` and `enabled`. Paths must be absolute. Existing library selections
cannot change during deployment. Fresh defaults use the Conatus state root,
the installed Annals state root and its `decisions/config.toml`, and library
`conatus`. Supply other selections explicitly. An absent schedule remains
absent unless `enabled` is supplied. Existing bindings retain their timer,
arguments, environment and output paths while selecting the new exact program
disabled. Activation restores enabled intent and the captured product pause
only after all holds release. No deployment clears a Clockwork incident.

`conatus --json config` reads only persistent dependency and library selections.
`conatus maintenance status|drain`, `hold OWNER` and `release OWNER` expose its
owner-scoped admission gate. Holds survive interruption. Recovery completes
configuration for a coherent selected release before releasing its own hold.

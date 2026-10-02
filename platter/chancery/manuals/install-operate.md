# Install and maintain Platter

Installation places resources and runs the declared product setup command under
ordinary admission and activity locks. It does not gate completion on artifact
integrity audits, or runtime readiness checks. The product's ordinary diagnostics
and runtime checks remain available separately.

Use this operation to inspect or change the matched Platter program/provider
release, verify readiness, maintain admission, or migrate supported state.
Platter owns domain state and compatibility. Cell coordinates selected releases;
Nucleus owns execution. Installation starts no preparation or email.

Read `nucleus manual` before coordinated maintenance. Read
`chancery resolve platter.install.operate` for this procedure and its required
feature contracts. `platter.maintenance` owns installation, hold/drain,
migration, schedule-definition, and recovery guarantees.
`platter.materials` owns templates and configuration; `platter.preparation`
owns readiness and prompt captures; `platter.editions` owns delivery authority.

## Plan and prepare a candidate

1. Identify the exact intended change and its supported state compatibility.
2. Inspect the canonical state root, current installation, required dependency
   releases, and maintenance owners.
3. Select the supported Cell delivery route and applicable installation authority.
4. Preserve private state outside the source repository.

Use the CI manager for ordinary committed delivery:

```sh
cell-ci submit COMMIT
```

The manager integrates, validates, attempts bounded repairs, deploys, and emails
the outcome. A separate release build prepares artifacts without installing or
running domain work:

```sh
python3 deployment/build.py --source-root /absolute/cell \
  --product platter --output /absolute/cell-build
```

For separately authorized installation from committed local `main`:

```sh
./deploy.sh plan platter
./deploy.sh platter
```

`plan` is read-only. Deployment selects the exact local main commit and ignores
uncommitted edits. Direct `platter-install install` and `recover` are refused;
use the instruction executor. Select required dependency updates explicitly.
The committed declarations order selected instructions; the executor adds no
dependencies or affected requesters. Unselected products retain their installed
interfaces. Deployment creates no maintenance hold and drains no requester.

Require Cast collection contract 5, Email's byte-payload interface, fixed Annals
Vita reads, compatible Weaver caller identity, authenticated Nucleus, renderer
tools, and a complete Bazaar prompt selection. Read `platter.maintenance` and
`platter.preparation` for exact readiness limits. A successful build or catalog
entry does not prove live readiness or authorize dependency upgrades.

## Inspect installation and readiness

```sh
platter-install inspect
platter --json doctor
platter --json doctor --state-only
```

`inspect` accepts `--home ABSOLUTE_PATH` and reads release metadata. Installation
does not run doctor or audit artifact integrity. Separately, full doctor checks retained state,
configured PDF and documented command/runtime prerequisites. It collects no
jobs, reads no Vita works, renders no PDF, creates no model job, and sends no mail.
Executable identity alone does not prove initialized Cast or Vita libraries.
State-only verification needs no renderer or external service readiness.

Stop for foreign selectors, ambiguous state roots, failed setup or migration,
or unknown ownership. Do not bypass these conditions with another state directory.

## Hold, drain, migrate, and verify

Use one retained owner identity throughout the authorized maintenance run:

```sh
platter --json maintenance status
platter --json maintenance hold OWNER
CELL_DEPLOYMENT_RUN_ID=OWNER platter --json maintenance drain
CELL_DEPLOYMENT_RUN_ID=OWNER platter --json migrate
platter --json maintenance release OWNER
```

1. Hold affected requester admission before holding Nucleus.
2. Observe actual local and exact matching Nucleus/Weaver work drain.
3. Require the sole matching owner and activity locks before migration or cutover.
4. Run migration under the sole matching owner hold.
5. Configure dependency paths and publish the candidate.
6. Release only this operation's hold and restore captured activation intent.
   Release Nucleus last.

Holds are durable and do not expire. Foreign holds and incomplete or failed
observations prevent drain and cutover. The 60-second observation bound does not
prove success when it expires. Drain includes only recorded Platter and matching
Weaver jobs; unrelated Weaver work stays outside Platter's cancellation authority.

Drain can cancel orphaned matching jobs only after local admissions and the
predecessor runner settle. Its scope is `platter` and `job-packets`, plus only
the exact Weaver job IDs retained in Platter runs. It creates no replacement
model attempt or synthetic domain record. Preserve holds if any matching job,
local activity, or other owner remains unresolved.

These commands are explicit maintenance operations. The manifest deployment
command does not invoke hold or drain and does not operate Nucleus maintenance.

Schema-one import commits before hashed file cleanup. Failure before commit
leaves predecessor state; failure afterward retains new state, originals, and
recovery information. Resume cleanup only with agreeing source hashes. Schemas
two through six migrate retained selections and advance to seven. Every
migration preserves captured inputs, requests, artifacts, and delivery identities.

Older binaries cannot operate schema seven. Recovery needs a compatible
candidate. Program selection does not undo migration. Do not reset records to
force success.

## Complete an interrupted migration

1. Retain the exact explicit maintenance owner, corrected compatible
   executable, and private completion-receipt path.
2. Exclude concurrent deployment through the executor's admission lock.
3. Require the existing sole hold, drained work, and activity locks.
4. Complete migration and local verification with the recorded evidence:

```sh
CELL_DEPLOYMENT_RUN_ID=OWNER /absolute/corrected/platter --json migrate \
  --completion-receipt /absolute/private/deployment/platter-migration.json
```

5. Inspect current state and complete the explicit maintenance operation after success.

The optional product completion receipt prevents this explicit operation from
repeating completed migration. Manifest deployment does not create or consume
it. Existing schema-one receipts remain accepted without reading their
former data-copy fields. This command does not rebind dependencies, release
holds, or establish full readiness. Omit `--completion-receipt` for ordinary
migration. Keep unresolved recovery held; do not reset domain state.

## Configure future work and preserve activation intent

Deployment initializes a missing template only from an explicit `resume`
absolute path. It cannot replace an initialized template. Optional
`projects_template` imports a new template through the Projects-only checks.
Deployment updates final Cast, Email, and Weaver executable references and
preserves the flat optional `resume_override`. Read `platter --json config`
without loading dependency data or preparing packets. Set or clear a daily PDF
with the configuration procedure in `platter.packet.prepare`.

Career reads remain fixed to `~/.local/bin/annals` and the library named `vita`.
Retired `crm_executable` fields are ignored and omitted when configuration is
saved. No Vita source setting is stored. Deployment supplies no missing prompt
contents. Before a caller uses separate project editorial policy, publish its
complete Bazaar components and selection as required by `platter.maintenance`.

The product command captures an existing `platter/daily` binding and selects its
updated definition directly. It preserves timer, arguments, environment,
working directory, output paths, incidents, and saved enabled intent.
An absent binding stays absent unless explicit activation settings are supplied.
Optional `enabled` overrides saved activation intent. Deployment temporarily
disables no binding and never clears a failure halt or reconciles a send.

For separately authorized daily activation, prepare a private definition:

```sh
umask 077
platter schedule-definition > /absolute/private/platter-daily.toml
```

Review it and use `clockwork.schedule.operate` for registration, binding changes,
and recovery. Printing starts no schedule and changes no binding. The ordinary
definition is daily 18:00 machine local time, run-at-load false, skip-on-overlap,
and `halt-until-approved`; Platter's configured zone selects edition dates.
Inspect the actual installed binding and process outcomes:

```sh
clockwork binding show platter/daily
clockwork history platter/daily --limit 20
clockwork incident list platter/daily
```

Platter's retained edition and receipt establish submission acceptance.
Clockwork history establishes process outcomes. Disable the binding to stop
future activations while retaining history. After repair, explicitly resume the
exact incident through Clockwork. Binding changes and deployment preserve halts;
resume permits future scheduling without retrying preparation or uncertain mail.

## Complete installation

Read the retained executor outcome for resource placement and product setup.
A failed instruction retains completed file, migration, configuration, and
schedule effects. The executor performs no automatic retry, rollback, or product
recovery. Inspect current Platter state before an authorized new attempt.
Reconcile and acknowledge executor admission separately from product recovery.
Preserve separately acquired maintenance holds.
Register command inventory after installation or update:

```sh
platter --register-usage
```

This starts no product work. Treat state, configuration, and logs as private.
Nucleus evidence and credentials remain separately owned. Domain artifacts have
no automatic pruning.
No installation-latency or arbitrary incompatible rollback guarantee is supplied.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

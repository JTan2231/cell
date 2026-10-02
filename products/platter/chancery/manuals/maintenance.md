# Maintenance and release

Use this feature to understand Platter's installed identity, canonical state,
readiness, admission holds, drain, migration, and recovery guarantees. Use
`platter.install.operate` for installation and maintenance procedures. Read
`platter.materials`, `platter.preparation`, and `platter.editions` for domain
meaning and delivery authority.

## Primary state interfaces

```sh
platter --json doctor
platter --json doctor --state-only
platter --json maintenance status
platter --json maintenance hold OWNER
CELL_DEPLOYMENT_RUN_ID=OWNER platter --json maintenance drain
CELL_DEPLOYMENT_RUN_ID=OWNER platter --json migrate
platter --json maintenance release OWNER
platter schedule-definition
```

Doctor and status inspect their documented scope without domain work. A hold
fences new mutations; drain can cancel exact matching orphaned work after local
activity settles. Migration requires the sole matching owner and proved drain.
Release removes only its named hold. Definition printing starts no schedule.
Read the guarantees below before using these interfaces; the ordered cutover and
recovery procedure belongs to `platter.install.operate`.

## Installed release and owned selectors

Platter's Rust installer packages the `platter` command, `platter-install`
recovery executable and matching Chancery provider as one immutable release.
Use the Cell deployment coordinator to change the installation. Direct
`platter-install install` and `recover` are refused because replacing this
requester must preserve admission, pending Nucleus work and domain state.

The read-only Cell deployment plan reports the intended operation. Cutover
selects an exact committed local `main` source and ignores uncommitted edits.
It creates no source release, commit, tag, or push. The caller selects required
product and dependency updates explicitly. Committed declarations order that
selection. The executor adds no products or requester maintenance. The supported
CI, build, deployment, and recovery procedures belong to `platter.install.operate`.

Cast must supply its supported retained export. URL-selected runs require one
matching retained job. Email must support `--payload-stdin` before Platter can
send database artifacts. Tectonic, Python 3 with pypdf, supported source data and compatible
authenticated Nucleus remain separate prerequisites.

Immutable installation files and prior releases remain beneath
`~/Library/Application Support/Platter/install/releases/ID`. The
`cell-install-v3` manifest records exact executable and provider versions,
file modes and public entry mappings. An opaque UUID identifies each prepared
release. `package/install` retains the
installer. The owned `current` selector publishes the matching
`~/.local/bin/platter`, `~/.local/bin/platter-install` and Chancery
`providers/platter` selector together. Manifests contain no private domain
content. Deployment does not prune retained product releases.
Foreign selectors stop publication; installation does not audit release bytes.
Product and catalog writer locks serialize individual atomic selector changes.
Failed publication retains completed changes without automatic compensation.

All durable runtime content and maintenance holds live in schema-seven
`packets.sqlite3` at the canonical root. Fresh state uses
`~/.local/share/platter`; a sole `~/.local/share/job-packets` predecessor remains
in place. Both roots are ambiguous and refused. An explicit `--state-dir` must
match the canonical root; independent custom live libraries are unsupported.
The database has private mode 0600 inside a private directory. SQLite recovery
journals remain beside it. Disposable renderer files and caches are confined
to that directory and removed after rendering; this is not memory-only LaTeX.

Configuration, original templates, captured inputs, compact execution records,
accepted artifacts, PDFs, frozen editions, explicit job eligibility, send
receipts and hold owners remain in SQLite. Platter retains no second tool-call
ledger. Nucleus's evidence and credentials remain separately owned.

Disposable Ashby board caches live outside SQLite under the same private runtime
root. They are not required for packet history. `platter.preparation` owns their
freshness, size, failure, and download-time semantics.

## Admission, drain, and migration

Owner holds are durable database rows and do not expire. A process holds an
advisory activity lock on the state directory for its entire mutating
command. Holds prevent admission while allowing existing work to finish.
The product deployment command uses ordinary admission and activity locks.
An existing hold can refuse setup. Explicit owner-scoped migration uses the
sole matching hold and proved drain. The predecessor maintenance gate and
runner lock remain observed while present. Explicit release can retire empty
predecessor gate files after drain.

Maintenance reads job-summary pages for `platter` and `job-packets`. When packet
executions retain Weaver assignments, it also reads `weaver` summaries and
matches only those exact retained job IDs. It does not read full job output for
each historical assignment. Unrelated Weaver jobs remain outside its authority.
Each maintenance observation is bounded to 60 seconds; an incomplete or failed
observation cannot establish drain.

Drain cancels orphaned matching work only once local admissions and the
predecessor runner have settled. It creates no replacement jobs or synthetic
domain records. Unresolved jobs and other hold owners prevent cutover.
Requester holds/draining precede Nucleus's hold, and Nucleus is released last.

Manifest deployment creates no maintenance hold, invokes no drain, and does not
release existing holds. Explicit maintenance commands retain their ordinary
admission and ownership rules. Installation requires no post-publication
readiness observation.

Migration is an explicit one-way schema-one to schema-seven import. It preserves
packet IDs as run IDs, captured bytes, exact Nucleus requests, frozen subjects,
bodies, attachment names/order, idempotency keys and acceptance/uncertainty.
Legacy reserved/sent jobs become ineligible; their preparation runs remain
ready. Test occurrences become edition rows without a type discriminator and
do not determine job eligibility. Any remaining owned runtime files are
retained as imported artifacts. Duplicate tool history is not imported.

The import commits transactionally before filesystem cleanup. It records a
hashed cleanup manifest in the same transaction. A missing, conflicting or
changed source file stops import. Cleanup failure retains originals and recovery
information. Reinvocation verifies every remaining source hash before removing
only manifest files. No production migration is implied by a source edit.

Old binaries cannot operate schema seven. Recovery after this boundary requires
a compatible candidate. Program selection does not undo schema
migration. Preserve holds after unresolved recovery.

Schema-two through schema-six migration adds ordered `edition_packets` records
from the existing attachment-to-run references and advances the database version.
It preserves exact selections, captured inputs, requests and artifact bytes. Existing runs
keep their legacy workflow. The version guard prevents an older binary from
interpreting a daily brief as a complete tailored packet or deriving packet
selection from the shared attachment. Migration starts no model work.

Migration verifies retained local state. It does not check dependency readiness
before deployment updates retained executable paths. The deployment command
does not run doctor or full readiness verification after setup.

## Interrupted migration completion

To complete an interrupted migration with a corrected compatible Platter
executable, retain the exact explicit maintenance owner and private completion
receipt path. Exclude concurrent deployment through its admission lock.
Run the corrected command under the existing sole owner hold:

```sh
CELL_DEPLOYMENT_RUN_ID=OWNER /absolute/corrected/platter --json migrate \
  --completion-receipt /absolute/private/deployment/platter-migration.json
```

The command requires drained work and the existing activity locks. It completes
the migration and local state verification before it writes the explicitly
requested product completion receipt. Repetition uses that receipt to avoid repeating completed
migration. Existing schema-one completion receipts remain accepted without
reading their former data-copy fields. The command checks local state but does
not rebind dependencies, release holds or establish full deployment readiness.
Inspect current product state and complete the explicit maintenance operation
after success. Manifest deployment does not create or consume a completion
receipt. Omit `--completion-receipt` for ordinary migration.

## Runtime diagnosis and installation inspection

Installation does not invoke doctor. As a separate runtime diagnostic,
`doctor` validates a configured resume override and checks retained state,
Cast/Annals/Email/Weaver executable identities, Email's byte-payload interface,
renderer availability and strict authenticated Nucleus readiness. Renderer
overrides are absolute `PLATTER_TECTONIC` and
`PLATTER_PYTHON`; fallback search is `~/.local/bin`, `/usr/local/bin`,
`/opt/homebrew/bin`, `/usr/bin`. These checks do not collect jobs, read Vita
works, render a PDF, submit a model job or send mail. Cast/Annals executable
identity is not proof that their libraries are initialized. `--state-only`
requires neither rendering nor external service readiness.

Read-only installation interfaces remain:

```sh
platter-install inspect
```

`inspect` accepts `--home ABSOLUTE_PATH` and reads installation metadata.
`platter-install deploy` is the product command invoked by the committed
manifest with JSON file references and settings on stdin. It selects program
files, migrates or initializes state, imports explicitly supplied templates,
updates dependency paths, and selects saved or explicit schedule intent.
The old adapter phase protocol is retired. Deployment performs no artifact
audit, automatic hold, drain, or application recovery.

Deployment initializes a missing resume only from an explicit setup path and
creates or enables a missing binding only from explicit activation settings. It
prepares no packets and sends no email. Domain artifacts
and accepted editions have no automatic pruning. Completed executor receipts and
logs remain retained; interrupted execution preserves working material. Product
releases and live state are not pruned by deployment.

## Daily definition and retained activation intent

With user authorization for recurring daily emails, Clockwork can activate the
verified release's `bin/platter` with the literal argument `run-daily`. Use the
installed `clockwork.schedule.operate` contract for definition registration,
binding changes and recovery. The `platter/daily` binding uses a daily local
calendar trigger at 18:00, run-at-load false, and skip-on-overlap. The Platter
configuration time zone determines the edition date; Clockwork's trigger
follows the machine zone. Cell deployment updates an existing binding and preserves its activation intent.

Deployment builds the schedule from selected release metadata and configuration.
It does not invoke the public command's release-integrity check.

`platter schedule-definition` prints the product-owned schema-two TOML from
the verified selected executable and configuration. It declares the default
`halt-until-approved` policy and uses Platter's configured Email wrapper for
incident notifications. It captures absolute renderer overrides when supplied,
uses the canonical state root as the working directory, and selects distinct
`daily.stdout.log` and `daily.stderr.log` files there. It creates no files,
registers no definition, and changes no binding. To prepare a private manifest:

```sh
umask 077
platter schedule-definition > /absolute/private/platter-daily.toml
```

Review and register that definition through Clockwork, preserving any intended
existing timer, environment and output-path configuration. Clockwork retains
the failure halt independently of definition selection and enabled state.

The product deployment command captures the prior digest and enabled state and
selects an updated definition directly. It preserves timer, environment,
working directory, private output paths, and failure incidents. It temporarily
disables no binding. Omitted `enabled` preserves saved intent.

Inspect `clockwork binding show platter/daily` and its selected definition for
the actual schedule. `clockwork history platter/daily --limit 20` reports
process outcomes; Platter's retained edition and receipt establish submission
acceptance. `clockwork binding disable platter/daily` stops future activations
and retains the definition and history. Uncertain sends stay held under the
preparation contract; changing a binding does not authorize another send.
Inspect `clockwork incident list platter/daily` after a failure. Once the cause
is resolved, `clockwork binding resume platter/daily INCIDENT_ID` explicitly
permits future scheduling. It creates no preparation attempt and does not
reconcile an uncertain edition. No deployment step clears this incident.

The product deployment command captures `platter/daily`. During setup it
migrates supported state, initializes a missing template from the supplied
`resume` absolute path, and updates Cast, Email and Weaver executable references
to the final installed releases. The `resume` setting cannot replace an initialized template. The optional
`projects_template` absolute path imports a private template through the same
projects-only checks as `platter import-projects-template`. Import retains the
old artifact and changes only the default template for future runs. `platter --json config` reads retained settings
without loading dependency data or preparing packets. Set or clear the flat
`resume_override` path with `platter config --resume-override ABSOLUTE_PDF` or
`platter config --clear-resume-override`. Deployment preserves this setting.
The preparation contract defines validation, daily scope and frozen-file behavior. Career reads use the fixed
`~/.local/bin/annals` command and its named `vita` library. Annals must support
named libraries and work list/show. Stored `crm_executable` fields are ignored
and omitted when configuration is saved. No Vita source setting is stored.

The optional `enabled` setting selects intended activation. An absent binding
stays absent when no activation setting is supplied. Existing definitions keep
their schedule, arguments, renderer environment and output paths and select
the new exact Platter program with saved or explicit enabled intent. It never
clears a Clockwork halt or reconciles a send.

A failed deployment retains completed file, migration, configuration, and
schedule effects. The executor stops and performs no automatic retry, rollback,
or product recovery. Reconcile an interrupted receipt and inspect current
product state before acknowledging executor admission or making a new attempt.
The product migration retains its transactional import and hashed source-cleanup
rules. Old data-copy files remain untouched.

## Weaver readiness boundary

Require Weaver authoring contract 2 and its caller-supplied request identity.
Platter pins the selected installed Weaver executable. Doctor checks that
`weaver write --help` exposes `--id` and, for initialized Platter state, checks
Weaver's read-only doctor. This creates no model job. Weaver must be deployed
before using the new Platter workflow. The caller selects updates explicitly;
the committed declarations order selected instructions. No unrelated Weaver jobs belong
to Platter's cancellation or recovery authority.

## Prompt prerequisites

Prompt preparation and exact selection behavior belong to `platter.preparation`.
Installation never supplies missing Bazaar contents. Before installing a caller
that uses the separate project editorial policy, publish
`platter.projects.editorial`, `platter.projects.cell.direction`,
`platter.projects.wrought.direction`, and `platter.projects.shorten.direction`.
Append a complete `cell.prompts.platter` selection with these IDs and all existing
entries. Keep selection version 1 and its components unchanged. Jackson uses
`platter.resume.editorial`; historical runs retain their captured directions.
Use the supported Bazaar update operation for text and selection publication.

## Privacy and limits

Installation manifests contain executable/provider identity and local paths,
not private resume bodies or provider credentials. Runtime state
remains private and separate from public immutable release material.
Installation does not establish send, recurring-delivery, or dependency-upgrade
authority. There is no installation-latency guarantee, arbitrary incompatible
rollback promise, future support interval, or deprecation window.

After each installation or update, run `platter --register-usage`.
This registers command inventory without product work.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

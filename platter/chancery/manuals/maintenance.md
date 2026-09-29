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
CELL_DEPLOYMENT_RUN_ID=OWNER platter --json migrate --backup /absolute/private/backup.sqlite3
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
It creates no source release, commit, tag, or push. When selected together,
Cast, Annals, Email, Nucleus, and Weaver install before Platter. Maintenance
includes Nucleus and affected requesters. Unselected products require compatible
installed maintenance interfaces and are not implicitly upgraded. The supported
CI, build, deployment, and recovery procedures belong to `platter.install.operate`.

Cast must support collection contract 5 for `job collect`: exact-job retention
with disabled-source access and preserved ordinary collection policy. Email must
support `--payload-stdin` before Platter can send database artifacts. Tectonic,
Python 3 with pypdf, supported source data and compatible
authenticated Nucleus remain separate prerequisites.

Immutable installation files and prior releases remain beneath
`~/Library/Application Support/Platter/install/releases/HASH`. The
`cell-install-v2` manifest records exact executable and provider versions,
file modes, digests and public entry mappings. `package/install` retains the
installer. The owned `current` selector publishes the matching
`~/.local/bin/platter`, `~/.local/bin/platter-install` and Chancery
`providers/platter` selector together. Manifests contain no private domain
content. Cell cleanup follows separate rules for unreferenced release history.
Altered releases, foreign selectors or changed candidate identities stop
publication. Product and catalog writer locks protect atomic selection and
file compensation.

All durable runtime content and maintenance holds live in schema-seven
`packets.sqlite3` at the canonical root. Fresh state uses
`~/.local/share/platter`; a sole `~/.local/share/job-packets` predecessor remains
in place. Both roots are ambiguous and refused. An explicit `--state-dir` must
match the canonical root; independent custom live libraries are unsupported.
The database has private mode 0600 inside a private directory. SQLite recovery
journals remain beside it. Disposable renderer files and caches are confined
to that directory and removed after rendering; this is not memory-only LaTeX.

Configuration, original template, captured inputs, compact execution records,
accepted artifacts, PDFs, frozen editions, explicit job eligibility, send
receipts and hold owners are covered by a consistent SQLite backup. Platter
retains no second tool-call ledger. Nucleus's evidence and credentials remain
separate and are not part of a Platter backup.

Disposable Ashby board caches live outside SQLite under the same private runtime
root. They are not required to restore packet history and are excluded from
database backups. `platter.preparation` owns their freshness, size, failure,
and download-time semantics.

## Admission, drain, and migration

Owner holds are durable database rows and do not expire. A process holds an
advisory activity lock on the state directory for its entire mutating
command. Holds prevent admission while allowing existing work to finish.
Installation admission requires the sole matching owner and drained local
activity. The predecessor maintenance gate and runner lock are also observed
while present, so a coordinated transition accounts for old binaries already
running. Empty predecessor gate files are retired on a drained final release.

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

For selected Platter, the coordinator can use its sealed candidate for
maintenance before publication when a read-only check proves schema-seven state.
This permits a corrected observer to replace a slow installed observer without
changing the public selection first. The installer still proves ownership of
the current installation before this choice. Foreign selectors, changed
candidate bytes, and unsupported state stop the operation.

Candidate maintenance uses the existing protocol, durable owners, activity locks,
and drain rules. It does not migrate state, prepare packets, or send mail. The
same choice applies during recovery and release before publication. Supported
predecessor schemas and existing affected-only Platter use the installed command.
Actual installed-program identity and readiness remain required after publication.

Migration is an explicit one-way schema-one to schema-seven import. It preserves
packet IDs as run IDs, captured bytes, exact Nucleus requests, frozen subjects,
bodies, attachment names/order, idempotency keys and acceptance/uncertainty.
Legacy reserved/sent jobs become ineligible; their preparation runs remain
ready. Test occurrences become edition rows without a type discriminator and
do not determine job eligibility. Any remaining owned runtime files are
retained as imported artifacts. Duplicate tool history is not imported.

The import commits transactionally before filesystem cleanup. It then writes
a complete schema-seven backup and records a hashed cleanup manifest. A missing,
conflicting or changed source file stops import; backup or cleanup failure
retains originals and recovery information. Reinvocation resumes cleanup only
when the chosen backup and remaining source hashes still agree. Only manifest
files are removed. A backup created here is a schema-seven recovery image, not an
old-binary rollback image. No production migration is implied by a source edit.

Old binaries cannot operate schema seven. Do not restore an old binary against
the migrated database. Recovery after this boundary requires a compatible
candidate or an explicitly selected complete predecessor database/files backup
with its matching binary. Installation file compensation does not undo schema
migration. Preserve holds after unresolved recovery.

Schema-two through schema-six migration adds ordered `edition_packets` records
from the existing attachment-to-run references and advances the database version.
It preserves exact selections, captured inputs, requests and artifact bytes. Existing runs
keep their legacy workflow. The version guard prevents an older binary from
interpreting a daily brief as a complete tailored packet or deriving packet
selection from the shared attachment. Each
migration retains a complete current-schema recovery backup. It does not create
an old-binary rollback image or start model work. Select a new backup path
when a retained backup uses a predecessor schema.

Migration verifies retained local state. It does not check dependency readiness
before deployment updates the retained executable paths. The coordinator runs
full readiness verification after configuration.

## Interrupted migration completion

To complete an interrupted migration with a corrected compatible Platter
executable, retain the exact deployment owner, backup and private completion
receipt path. Stop concurrent coordinator recovery through its deployment lock.
Run the corrected command under the existing sole owner hold:

```sh
CELL_DEPLOYMENT_RUN_ID=OWNER /absolute/corrected/platter --json migrate \
  --backup /absolute/private/backup.sqlite3 \
  --completion-receipt /absolute/private/deployment/platter-migration.json
```

The command requires drained work and the existing activity locks. It completes
the migration and local state verification before it writes the coordinator's
completion receipt. The receipt binds the exact backup path and digest. A repeat
verifies that evidence and local state; changed evidence stops recovery. An
existing backup remains unchanged. The command does not rebind dependencies,
release holds or establish full deployment readiness. Resume coordinator recovery
after the command succeeds. Omit `--completion-receipt` for ordinary migration.

## Readiness and installation verification

`doctor` validates a configured resume override and checks retained state,
Cast/Annals/Email/Weaver executable identities, Cast's exact
job-URL command, Email's byte-payload interface, renderer availability and
strict authenticated Nucleus readiness. Renderer overrides are absolute `PLATTER_TECTONIC` and
`PLATTER_PYTHON`; fallback search is `~/.local/bin`, `/usr/local/bin`,
`/opt/homebrew/bin`, `/usr/bin`. These checks do not collect jobs, read Vita
works, render a PDF, submit a model job or send mail. Cast/Annals executable
identity is not proof that their libraries are initialized. `--state-only`
requires neither rendering nor external service readiness.

Read-only installation interfaces remain:

```sh
platter-install inspect
platter-install verify --binary /absolute/candidate/platter --bundle /absolute/cell/platter/chancery
platter-install verify-release /absolute/owned/release
```

`inspect` and `verify` accept `--home ABSOLUTE_PATH`. `verify` compares the
installed release with the candidate and executing installer. `verify-release`
checks retained release integrity without changing selectors. The sealed version-one
`platter-install adapter OP` remains the coordinator boundary for inspect,
hold, drain, apply, verify, release and recover. Apply requires exact run-owned
maintenance. Candidate and source material are verified; affected-only products
are not upgraded. Interrupted or unsafe recovery retains its owner hold.

Deployment initializes a missing resume only from an explicit setup path and
creates or enables a missing binding only from explicit activation settings. It
prepares no packets and sends no email. Domain artifacts
and accepted editions have no automatic pruning. Candidate workspaces and
installation release history remain under Cell's separate retention rules.

## Daily definition and retained activation intent

With user authorization for recurring daily emails, Clockwork can activate the
verified release's `bin/platter` with the literal argument `run-daily`. Use the
installed `clockwork.schedule.operate` contract for definition registration,
binding changes and recovery. The `platter/daily` binding uses a daily local
calendar trigger at 18:00, run-at-load false, and skip-on-overlap. The Platter
configuration time zone determines the edition date; Clockwork's trigger
follows the machine zone. Cell deployment updates an existing binding and preserves its activation intent.

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

Cell deployment captures the prior digest and enabled state, disables the
binding under maintenance, and selects an updated exact definition disabled.
It preserves timer, environment, working directory and private output paths.
Activation restores the intended state after all holds release. Recovery
retains the prior evidence and selects only a coherent installed release.

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

Cell deployment captures and disables `platter/daily`. During configuration it
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
the new exact Platter program disabled. Final activation restores intent after
all holds release. It never clears a Clockwork halt or reconciles a send.

Each deployment retains its own migration backup. A new backup is selected
only after any prior import cleanup completes and its retained backup digest
remains valid. Repeating an interrupted run reuses its validated backup.

## Weaver readiness boundary

Require Weaver authoring contract 2 and its caller-supplied request identity.
Platter pins the selected installed Weaver executable. Doctor checks that
`weaver write --help` exposes `--id` and, for initialized Platter state, checks
Weaver's read-only doctor. This creates no model job. Weaver must be deployed
before the new Platter release. The shared deployment dependency declaration
orders the selected releases and maintenance. No unrelated Weaver jobs belong
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
not private resume bodies or provider credentials. Runtime state and backups
remain private and separate from public immutable release material.
Installation does not establish send, recurring-delivery, or dependency-upgrade
authority. There is no installation-latency guarantee, arbitrary incompatible
rollback promise, future support interval, or deprecation window.

After each installation or update, run `platter --register-usage`.
This registers command inventory without product work.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

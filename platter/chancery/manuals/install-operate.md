# Install and maintain Platter

Platter's Rust installer packages the `platter` command, `platter-install`
recovery executable and matching Chancery provider as one immutable release.
Use the Cell deployment coordinator to change the installation. Direct
`platter-install install` and `recover` are refused because replacing this
requester must preserve admission, pending Nucleus work and domain state.

Building a candidate does not install it or run domain work:

```sh
./ci.sh platter
python3 deployment/build.py --source-root /absolute/cell \
  --product platter --output /absolute/cell-build
```

When installation is authorized and the changes are committed on local main:

```sh
./deploy.sh plan platter
./deploy.sh platter
```

`plan` is read-only. A deployment selects its exact local `main` commit; it
ignores uncommitted changes and does not publish a release, commit, tag or
push. When selected together, Cast, CRM, Email and Nucleus install before
Platter. Maintenance includes Nucleus and its registered requesters, whose
installed maintenance interfaces must already be compatible. Unselected
products are not upgraded to satisfy a missing prerequisite.

Cast must support collection contract 5 for `job collect`: exact-job retention
with disabled-source access and preserved ordinary collection policy. Email must
support `--payload-stdin` before Platter can send database artifacts. Tectonic,
Python 3 with pypdf, supported source data and compatible
authenticated Nucleus remain separate prerequisites.

## Owned storage

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

All durable runtime content and maintenance holds live in schema-two
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

Disposable Ashby board files live in `ashby-cache/BOARD.json` under the same
runtime root, outside SQLite. Preparation and preview share each download for
less than 14 days and refresh it when the selected posting is absent. Invalid
or expired caches are also refreshed on demand. A failed refresh preserves the
old file but fails retrieval. Cache files are private, can be removed to force
the next download, and are not required to restore packet history. They have
no byte cap and are not included in database backups. The selected posting and
its original download time remain captured in each preparation run.

## Maintenance and schema migration

```sh
platter --json maintenance status
platter --json maintenance hold OWNER
CELL_DEPLOYMENT_RUN_ID=OWNER platter --json maintenance drain
CELL_DEPLOYMENT_RUN_ID=OWNER platter --json migrate --backup /absolute/private/backup.sqlite3
platter --json doctor
platter --json doctor --state-only
platter --json maintenance release OWNER
```

Owner holds are durable database rows and do not expire. A process holds an
advisory activity lock on the state directory for its entire mutating
command. Holds prevent admission while allowing existing work to finish.
Installation admission requires the sole matching owner and drained local
activity. The predecessor maintenance gate and runner lock are also observed
while present, so a coordinated transition accounts for old binaries already
running. Empty predecessor gate files are retired on a drained final release.

Maintenance observes all pages of nonterminal jobs under both `platter` and
`job-packets`. Drain cancels orphaned matching work only once local admissions
and the predecessor runner have settled. It creates no replacement jobs or
synthetic domain records. Unresolved jobs and other hold owners prevent cutover.
Requester holds/draining precede Nucleus's hold, and Nucleus is released last.

Migration is an explicit one-way schema-one to schema-two import. It preserves
packet IDs as run IDs, captured bytes, exact Nucleus requests, frozen subjects,
bodies, attachment names/order, idempotency keys and acceptance/uncertainty.
Legacy reserved/sent jobs become ineligible; their preparation runs remain
ready. Test occurrences become edition rows without a type discriminator and
do not determine job eligibility. Any remaining owned runtime files are
retained as imported artifacts. Duplicate tool history is not imported.

The import commits transactionally before filesystem cleanup. It then writes
a complete schema-two backup and records a hashed cleanup manifest. A missing,
conflicting or changed source file stops import; backup or cleanup failure
retains originals and recovery information. Reinvocation resumes cleanup only
when the chosen backup and remaining source hashes still agree. Only manifest
files are removed. A backup created here is a schema-two recovery image, not an
old-binary rollback image. No production migration is implied by a source edit.

Old binaries cannot operate schema two. Do not restore an old binary against
the migrated database. Recovery after this boundary requires a compatible
candidate or an explicitly selected complete predecessor database/files backup
with its matching binary. Installation file compensation does not undo schema
migration. Preserve holds after unresolved recovery.

## Readiness and recovery

`doctor` checks retained state, configured executable identities, Cast's exact
job-URL command, Email's byte-payload interface, renderer availability and
strict authenticated Nucleus readiness. Renderer overrides are absolute `PLATTER_TECTONIC` and
`PLATTER_PYTHON`; fallback search is `~/.local/bin`, `/usr/local/bin`,
`/opt/homebrew/bin`, `/usr/bin`. These checks do not collect jobs, read CRM
profiles, render a PDF, submit a model job or send mail. Cast/CRM executable
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

## Separately managed daily activation

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

## Deployment setup and retained schedule intent

Cell deployment captures and disables `platter/daily`. During configuration it
migrates supported state, initializes a missing template from the supplied
`resume` absolute path, and updates Cast, CRM and Email executable references
to the final installed releases. An initialized template cannot be replaced
through deployment settings. `platter --json config` reads retained settings
without loading dependency data or preparing packets.

The optional `enabled` setting selects intended activation. An absent binding
stays absent when no activation setting is supplied. Existing definitions keep
their schedule, arguments, renderer environment and output paths and select
the new exact Platter program disabled. Final activation restores intent after
all holds release. It never clears a Clockwork halt or reconciles a send.

Each deployment retains its own migration backup. A new backup is selected
only after any prior import cleanup completes and its retained backup digest
remains valid. Repeating an interrupted run reuses its validated backup.

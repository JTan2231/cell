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

Email must support `--payload-stdin` before Platter can send database artifacts.
Tectonic, Python 3 with pypdf, supported source data and compatible
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

`doctor` checks retained state, configured executable identities, Email's
byte-payload interface, renderer availability and strict authenticated Nucleus
readiness. Renderer overrides are absolute `PLATTER_TECTONIC` and
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

No installation operation initializes a resume, prepares packets, sends email,
creates a Clockwork binding or enables recurring delivery. Domain artifacts
and accepted editions have no automatic pruning. Candidate workspaces and
installation release history remain under Cell's separate retention rules.

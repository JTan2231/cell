# Install and maintain Platter

Platter's Rust installer packages the `platter` command, `platter-install`
recovery executable and matching Chancery provider as one immutable release.
Its supported mutation route is the Cell deployment coordinator. Direct
`platter-install install` and `recover` are refused because replacing this
requester must preserve admission, pending Nucleus work and domain state.

Building a candidate is separate from installing it:

```sh
./ci.sh platter
python3 deployment/build.py --source-root /absolute/cell \
  --product platter --output /absolute/cell-build
```

The builder prepares `candidates/platter/bin/platter`,
`candidates/platter/bin/platter-install` and a sealed candidate manifest. It
builds code and verifies candidate material; it does not install commands,
prepare packets, render resumes, collect jobs, submit Nucleus jobs, send email
or enable a schedule. CI is the separate development check.

When installation has been authorized and the changes are committed on local
`main`, the coordinator interface is:

```sh
./deploy.sh plan platter
./deploy.sh platter
```

`plan` is read-only. A deployment selects its exact local `main` commit; it
ignores uncommitted changes and does not publish a release, commit, tag or
push. Platter orders selected Cast, CRM, Email and Nucleus installations before its
own cutover. The conservative
maintenance closure includes Nucleus and its registered requesters, whose
installed maintenance interfaces must already be compatible. Unselected
products are not upgraded to satisfy a missing prerequisite.

## Installed files and private state

Owned installation files are beneath
`~/Library/Application Support/Platter/install/releases/HASH`. The
`cell-install-v2` manifest records exact executable and provider versions,
file modes, digests and public entry mappings. `package/install` retains the
installer. The owned `current` selector publishes the matching
`~/.local/bin/platter`, `~/.local/bin/platter-install` and Chancery
`providers/platter` selector together. The installer retains prior releases for verified recovery. Cell coordinated
cleanup follows its separate rules for removing unreferenced release history.
Foreign selectors, altered retained files and mismatched candidate/provider
versions stop publication. Product and catalog writer locks protect the
atomic selection and file compensation.

Installation does not initialize a resume, create packets, or replace private
inputs. Fresh runtime state defaults to `~/.local/share/platter`. If only the
predecessor `~/.local/share/job-packets` exists, Platter uses it in place. If
both exist, CLI callers must explicitly choose `--state-dir ABSOLUTE_PATH`;
the coordinator refuses ambiguous default state. Existing absolute artifact
paths, packet IDs, exact Nucleus requests, tool receipts, edition keys and
ad hoc send receipts remain unchanged. New work uses requester program
`platter`; retained predecessor work keeps `job-packets` identity.

The private state schema remains 1. Unknown schemas, invalid original
resume snapshots and inconsistent retained state are refused. The original
resume, captured CRM material, stage inputs, PDFs, editions and receipts stay
outside the immutable install tree and the source repository. Back up the
whole runtime directory consistently while work is inactive; a database
backup alone does not contain the original resume or artifacts.

## Maintenance and readiness

The CLI provides these operational interfaces:

```sh
platter --json maintenance status
platter --json maintenance hold OWNER
CELL_DEPLOYMENT_RUN_ID=OWNER platter --json maintenance drain
platter --json maintenance release OWNER
platter --json doctor
CELL_DEPLOYMENT_RUN_ID=OWNER platter --json migrate \
  --backup /absolute/private/backup.sqlite3
```

`CELL_DEPLOYMENT_RUN_ID` must match the sole retained hold for drain and
migration; the coordinator supplies it. Ordinary status and owner hold/release
commands do not submit model work.

A per-user durable gate under
`~/Library/Application Support/Platter/deployment-maintenance` fences every
mutating Platter command, including callers selecting custom state. Owner
holds do not expire. Releasing one owner preserves other owners' holds.
Existing admitted commands can finish; the default-state runner lock also
accounts for a still-running predecessor CLI.

Status checks live admissions and nonterminal Nucleus work under both
`platter` and `job-packets` requester programs, following all result pages.
Once no live runner or admission remains, drain cancels orphaned matching
Nucleus jobs and waits for terminal state. It does not submit replacement
jobs or run requester tools. Accepted stages and receipts remain retained;
cancellation leaves any unaccepted attempt subject to the ordinary explicit
recovery limits. An unresolved job or competing hold prevents cutover.

The coordinator holds and drains requesters before Nucleus, applies the
selected candidate, verifies while held, then releases requester holds and
Nucleus last. `migrate --backup` requires the deployment hold and quiescence,
checks supported schema and captured template, and takes a private consistent
SQLite backup when a database exists. This release needs no schema change and
never initializes missing domain state as part of installation.

`doctor` checks existing local state and the configured Cast, CRM, Email,
renderer and Nucleus prerequisites without creating domain work. Email must
support repeated local `--attach` arguments; an older Email installation must
be upgraded separately when authorized. Renderer readiness requires Tectonic,
Python 3 and `pypdf`. `PLATTER_TECTONIC` and `PLATTER_PYTHON` may select absolute
executables. Otherwise resolution checks `~/.local/bin`, `/usr/local/bin`,
`/opt/homebrew/bin` and `/usr/bin` in that order, independent of the caller
PATH. Preparation uses the same resolver. Nucleus must have compatible authenticated harness
readiness; during deployment its exact run-owned hold is checked.
These probes do not call Cast collection, consume Cast API-key budgets, submit
model jobs, render a document, send email or read a provider account balance.
`doctor --state-only` proves retained schema and template compatibility without
execution, rendering or delivery prerequisites. Recovery of an unchanged prior
Platter installation, and verification of an affected-only prior installation,
use this check under the existing quiescence proof. Selected candidate
verification still requires full readiness.
Cast and CRM probes inspect executable identity only; they do not read
exports, profile contents or prove those libraries initialized. These checks
establish local readiness at observation time, not future source availability,
model success, final PDF fidelity or inbox receipt.

## Inspection and recovery

The installer exposes read-only verification separately from publication:

```sh
platter-install inspect
platter-install verify --binary /absolute/tested/platter \
  --bundle /absolute/cell/platter/chancery
platter-install verify-release /absolute/owned/release
```

`inspect` and `verify` accept an explicit `--home ABSOLUTE_PATH`. `verify`
compares the installed release with the supplied candidate and executing
installer. `verify-release` checks retained release integrity without changing
selectors. No prior installed Platter release format is supported; the
predecessor compatibility is runtime state compatibility, not an invented
legacy installation format.

The coordinator invokes the sealed `platter-install adapter OP` with its
version-one JSON request for `inspect`, `hold`, `drain`, `apply`, `verify`,
`release` or `recover`. This internal boundary verifies the candidate and
source material, captures prior selection, and rejects installation of an
affected-only product. Inspection is read-only. Apply requires the exact
run-owned drained hold; changing selection since inspection is refused.

On an ordinary publication failure, shared installation transactions restore
coherent prior selectors. Coordinated recovery proves a coherent unchanged
prior or exact candidate installation, supported domain state and readiness
before releasing holds. It does not blindly repeat an uncertain apply or
restore a database merely because an older binary exists. An unsafe or
interrupted recovery leaves maintenance held and reports its owner. Preserve
the held state and use a reviewed product recovery procedure; do not delete
holds or alter immutable releases to bypass the failed proof. There is no
supported arbitrary direct rollback or incompatible schema migration.

No installer operation creates a Clockwork binding, LaunchAgent, recurring
09:00 delivery or send authorization. Runtime artifacts and send history have
no automatic pruning. The install manifest excludes private resume content,
career entries and credentials. Candidate builds and coordinator workspaces
follow Cell's separate cache and temporary-file retention rules.

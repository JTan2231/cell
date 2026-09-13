# Install or operate CRM and its steward

Use this operation to install CRM, initialize, migrate or diagnose its database,
and recover hidden steward work from retained records. It does not authorize
release publication, production data mutation,
Nucleus changes, or an otherwise ineligible retry.

## Build before deployment

```sh
./crm/ci.sh
<TESTED_CRM_INSTALL> install \
  --binary <TESTED_CRM_BINARY> \
  --bundle /Users/joey/rust/cell/crm/chancery
```

The Rust installer requires a regular executable and complete provider bundle
at absolute paths, matching its own tested version. It checks that
the candidate version matches the packaged provider release, validates the
exact bundle/tree and component hashes, stages one immutable content-addressed
release, then publishes command and provider views through one current-release
selector. New releases retain `bin/crm-install` and `package/install`, with an
exact `cell-install-v2` inventory in `manifest.json`. The installer also verifies
the supported legacy release format.

Owned paths are:

```text
~/.local/bin/crm
~/.local/bin/crm-install
~/Library/Application Support/CRM/install/{current,previous,releases/}
~/Library/Application Support/Chancery/providers/crm
```

It refuses foreign, symbolic, traversal, fabricated, or tampered owned paths.
Identical redeployment is idempotent. An update retains the prior validated
release, and a failed post-switch smoke restores both public views before
commit.

A PID-aware product lock serializes CRM updates, followed by the shared
Chancery catalog-writer lock for publication. The deployer snapshots `current`
before waiting and rejects a stale cutover after either lock. A planner may
instead pass `--expected-current absent|releases/HASH`. The one `current`
replacement publishes command and provider together; failed smoke restores the
prior view or detaches the public selectors if coherent restoration cannot be
proved.

Deployment never creates or opens `crm.db`, launches a worker, restarts
Nucleus, or installs a daemon, LaunchAgent, or schedule.

## Initialize and diagnose

```sh
/Users/joey/.local/bin/crm init
/Users/joey/.local/bin/crm doctor
```

Initialization is a separate intentional effect. It creates schema two when
the selected file is absent and is idempotent against an existing supported CRM
database; it refuses symbolic links and other non-regular targets before
SQLite opens them, refuses foreign or unsupported schemas, and does not
migrate. New Unix database bytes are mode 0600. The packaged default state
directory is mode 0700, while a caller-selected relative database resolves
against the current working directory.

Doctor checks schema identity and the seven required tables, SQLite integrity,
foreign keys, secure database/sidecar permissions, strict Nucleus readiness,
and idempotent registration of the immutable `crm/case-steward/1` toolset.
Storage integrity belongs to CRM; execution
readiness belongs to Nucleus. A Nucleus failure does not make existing cases
unreadable, but hidden steward work cannot progress through a second path.

## Explicit schema-one migration

Schema two adds an empty `profile_entries` table; it does not import source
files, rewrite cases, or change requester/toolset meaning. Migration is a
separate user-intended effect from deployment or initialization:

```sh
/Users/joey/.local/bin/crm migrate --backup /absolute/private/path/crm-schema1.db
```

Select another database with global `--database` or `CRM_DATABASE` when
appropriate. Stop new CRM use and let active workers finish, including terminal
runtime settlement after an applied revision. Under an immediate write
transaction, migration refuses a live worker PID, a running update, or an
applied update with no terminal runtime evidence. Queued updates may remain
queued. Existing case, revision, delivery, receipt, queue, and lease rows are
preserved.

The backup path must be new, separate from the source database and its
sidecars, and have no existing SQLite sidecars. Its parent must already be a
private non-symbolic directory with no group/other permissions on Unix. A
relative backup path resolves against the current working directory. An
existing destination is refused.

CRM creates a private, independently readable SQLite snapshot that includes
committed WAL content, then adds the profile table and advances both schema
markers with integrity validation before commit. Failure before commit leaves
the source at schema one. The result reports `changed`, `from_schema_version`,
and `schema_version`, plus database and backup paths. `backup` is absolute
when changed and null otherwise.

Repeating on schema two is unchanged and creates no backup. After an ambiguous
result, inspect schema before retrying. A failed backup may leave its
destination; inspect it and choose a fresh destination for another attempt.
Migration calls no Nucleus service and starts no worker.

Retain the backup as private data. To roll back storage, stop all CRM use,
preserve the newer database and applicable sidecars, then restore the complete
schema-one backup without pairing it with schema-two WAL/SHM. Only then select
a compatible older binary. Post-backup writes will leave the active view, so
this recovery requires explicit authority and retention of the newer state;
program-selector rollback alone is insufficient.

## Inspect and recover hidden work

Begin with evidence:

```sh
/Users/joey/.local/bin/crm update show UPDATE_ID
/Users/joey/.local/bin/nucleus jobs list \
  --requester crm --requester-id case-steward:UPDATE_ID
```

Use CRM state to decide domain success and retry eligibility. Nucleus job state
is execution evidence. A terminal job without a CRM committed revision is not
success; a CRM commit remains success after a later harness failure. Update
views separately expose successful-result acknowledgment and the retained
terminal runtime state/detail.

For queued or exactly recoverable work:

```sh
/Users/joey/.local/bin/crm update resume UPDATE_ID
```

Resume processes the selected queued, recoverable running, or
applied-but-runtime-unsettled update synchronously and preserves its
requester/job identity. It can recover a durable pending tool call
idempotently, repost the exact committed result, and retain terminal execution
diagnostics. It cannot create a successor update merely because transport or
process state is ambiguous. If wait, resume, or retry fails after resolving an
update, JSON failure output includes a contextual update view with its
attention/advisory; human stderr prints the fixed nonblocking advisory banner
before the error.

For a failed or lost attempt with no committed revision:

```sh
/Users/joey/.local/bin/crm update retry UPDATE_ID
```

Retry reuses the same immutable delivery row/text in a successor update with
new requester/job identities, retains the predecessor through `retry_of`, and
launches the hidden worker. CRM performs no automatic retry.
A Nucleus restart may mark a live harness attempt lost; that is terminal
evidence for CRM to validate, not permission for Nucleus or an operator to
rewrite domain rows.

Hidden drainers serialize through a database-resident lease. A drainer launched
while another owns the lease waits for at most two seconds. The lease owner
performs an atomic queue handoff: a drain owner claims the next eligible update
or releases ownership, and a resume owner releases ownership and starts one
replacement drainer when eligible work is waiting. A bounded contender therefore
cannot open an empty-check/release race.

The hidden Codex invocation uses model `gpt-5.6-terra`, medium reasoning, a
1,200-second timeout, requester `crm`, requester ID
`case-steward:UPDATE_ID`, workspace access `none`, no shell or web, no launch
context, and exactly one managed tool under `crm/case-steward/1`. There is no
direct-Codex fallback.

## Verify a deployment

```sh
/Users/joey/.local/bin/crm --version
/Users/joey/.local/bin/crm doctor
/Users/joey/.local/bin/chancery show crm.case.maintain
/Users/joey/.local/bin/chancery show crm.library.explore
/Users/joey/.local/bin/chancery show crm.profile.maintain
/Users/joey/.local/bin/chancery show crm.steward.operate
/Users/joey/.local/bin/chancery doctor
/Users/joey/.local/bin/nucleus health
```

Never put real job-search, contact, prompt, or tool-result content in release
bytes, logs, or CI fixtures.

## Rollback

After a committed deployment, resolve `install/previous` to its canonical owned
release directory. Use a trusted tested Rust installer to verify and select it:

```sh
<TRUSTED_CRM_INSTALL> recover \
  --release <VERIFIED_PREVIOUS_RELEASE_DIRECTORY>
```

Do not execute an unverified installer from the retained release. Normal
ownership, exact-tree, manifest, hash, and version checks apply.
Rollback switches only program/provider selection. It never rewrites CRM or
Nucleus state. Stop when `previous` is absent/invalid or the older binary
cannot read the retained database schema; use that release's database-aware
recovery plan instead of forcing binary rollback.

CRM has no uninstaller or automatic pruning. Deleting retained cases,
attempts, releases, or a database is a separate destructive action.

## Rust callers

Use `crm::api::Client` with provider-owned request and response types.
The client invokes an explicitly selected CLI and decodes its envelopes. It
preserves this operation's effects, failures, and authority requirements and
does not retry automatically. Convert results only to caller-local models.

## Coordinated deployment maintenance

```sh
crm --json maintenance hold RUN_ID
crm --json maintenance status
crm --json maintenance drain
crm --json maintenance release RUN_ID
```

The selected database's parent directory contains `deployment-maintenance/`. The standard
location is `~/Library/Application Support/CRM/deployment-maintenance/`.
Databases in the same parent share this gate. This directory stores only
operational hold/lock metadata, never retained case or profile text.

Each hold is durable and has its own owner. While held, CRM rejects case and
profile mutations, tell, retry, ordinary initialization and migration.
Existing queued updates and running or applied-but-runtime-unsettled updates
remain recoverable through the original hidden worker, wait, resume, or
`maintenance drain`. Recovery holds a shared activity guard, so an installation
cannot race a hidden drainer. It never creates a replacement model attempt.

`--json` reports `data.maintenance` with `protocol_version: 1`, `holds`,
`drained`, `unsettled_updates`, and `worker_alive`. Drain requires zero queued,
running, or runtime-unsettled applied updates, no live worker lease, and no
admitted operation. Schema-one maintenance observation is supported before
its explicit migration. An unavailable or ambiguous worker is not assumed
settled. Release removes only its exact owner's hold.

The coordinator uses the program installer and the separate `migrate --backup`
command. Its backup destination is `~/Library/Application
Support/CRM/crm-pre-migration-RUN_ID.sqlite`, outside the temporary deployment
workspace. The migration creates this private backup only when schema
migration is needed; current-schema deployment creates no backup. A created
backup survives deployment cleanup, including an interrupted or failed
migration, for explicit database recovery.

Migration with `CELL_DEPLOYMENT_RUN_ID` acquires exclusive activity only when
the run owns the sole matching hold. An arbitrary environment value cannot
bypass another owner or active work. The program installer still never opens
or migrates CRM data. `doctor` can validate held Nucleus readiness for that
exact deployment owner; normal steward admission remains strict.

Deployment verification checks the installed release, database integrity,
Nucleus readiness, and settled maintenance status. It creates no case, update,
or model job. Ordinary CRM success remains the guarded revision commit; a
later runtime failure does not reverse that commit.

Deployment admission resolves the configured database to its canonical path and
uses that database parent for `deployment-maintenance/`. Symbolic aliases share
the same gate. Databases with multiple hard links are rejected because their
state root cannot identify one authoritative admission gate. Database paths
in command receipts use this canonical identity; backup receipts retain the
caller-selected backup path.

## Output selection

Update views include domain state, Nucleus correlation, runtime settlement,
result-post status, failures, and complete advisory and attention. Update list
defaults to 20 results with `has_more`; use `--limit` with a positive integer
for more. Show, wait, resume and retry return the complete selected update view.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.

# macOS user installation

CRM installs one short-lived CLI and one product-owned Chancery provider. It
has no daemon, LaunchAgent, scheduler, source crawler, contact sender, or direct
Codex runner. A hidden child worker is launched only for explicitly queued or
resumed update work and uses the separately installed Nucleus service.

Build and validate first:

```sh
./crm/ci.sh
cargo build --release --locked --package crm
crm/packaging/macos/deploy-user.sh \
  --binary /Users/joey/rust/cell/target/release/crm
```

Deployment publishes stable command and provider selectors through one
content-addressed current-release selector. It validates binary/provider
version, exact release tree, content manifest, selector ownership, installed
help/version, and pre-commit rollback. It does not initialize, open, migrate,
back up, inspect, or delete CRM state and does not restart Nucleus or launch a
worker.

A PID-aware product lock serializes CRM updates. Provider publication also
takes the shared Chancery catalog-writer lock, always after the product lock,
so generated selector-only deployers cannot publish concurrently; stale lock
owners are recovered. Deployment snapshots `current` before waiting and
rejects a stale cutover. Callers may instead supply
`--expected-current absent|releases/HASH`. The one `current` switch is atomic;
failed smoke checks restore the prior selector view or detach it if restoration
cannot be proved.

Initialize separately after a fresh deployment:

```sh
/Users/joey/.local/bin/crm init
/Users/joey/.local/bin/crm doctor
```

An existing schema-one database is a separate migration effect. Stop new CRM
work, let active workers and runtime settlement finish, and run:

```sh
/Users/joey/.local/bin/crm migrate --backup /absolute/private/path/crm-schema1.db
```

The backup path must be new. Migration preserves existing case and queue rows
and adds an empty profile table; it does not import source files. It refuses
live workers and unsettled active updates. See the [data model](data-model.md#initialization-integrity-and-migration)
for transaction, backup, and database rollback semantics. A program downgrade
to a schema-one release requires restoring the compatible backup separately;
selector rollback does not downgrade the database.

## Paths

```text
~/.local/bin/crm
~/Library/Application Support/CRM/crm.db
~/Library/Application Support/CRM/install/{current,previous,releases/}
~/Library/Application Support/Chancery/providers/crm
```

The database and SQLite sidecars are retained independently from installed
releases. Version 0.3 has no uninstaller or automatic pruning. Removing cases,
intake, steward updates, tool receipts, or retained releases is a separate
destructive action requiring explicit authority.

## Verify

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

Doctor checks schema identity/table presence, foreign keys, SQLite integrity,
secure database/sidecar permissions, and strict Nucleus/toolset readiness. It
does not
prove a source, case claim, contact decision, connection, or employment result.

## Acceptance canary

Use an isolated CRM database and synthetic case material. Initialize it, create
a research-stage case, record one delivery with `tell`, retain the returned
update identity, wait for the hidden worker, and prove:

1. the delivery and queued update were durable before worker success;
2. the Nucleus job used requester `crm`, requester identity
   `case-steward:UPDATE_ID`, and
   immutable toolset `crm/case-steward/1` with no workspace, shell, web, launch
   context, or second tool;
3. one accepted `submit_case_revision` call with the frozen base guard and
   four revision fields atomically created the immutable next revision and
   replay-safe receipt;
4. `case show`, `history`, `search`, and `update show` agree on the revision,
   stage, summary, advisory, base, and Nucleus correlation as exposed by their
   respective content and operational views; and
5. a non-null advisory is conspicuous on case reads, tell acknowledgment, and
   update list/show/wait/resume/retry, but does not make a valid operation fail
   merely because the advisory exists.

Also prove `failed` or `lost` work requires explicit retry, which creates a new
steward update, requester and job while retaining `retry_of`; Nucleus completion
without a committed revision is not accepted as CRM success. CI fixtures and
release bytes must be synthetic and contain no real contacts, job-search notes,
prompts, or tool results.

## Rollback

If deployment fails before commit, the deployer restores both binary and
provider views. After a committed update, preserve canary evidence and redeploy
the exact previous binary with its packaged deployer:

```sh
crm_previous="/Users/joey/Library/Application Support/CRM/install/previous"
"$crm_previous/package/deploy-user.sh" \
  --binary "$crm_previous/bin/crm"
```

Normal selector, exact-tree, manifest, component-hash, and version checks still
apply. Stop if `previous` is absent or invalid. Rollback changes program and
provider selection only; it never rewrites CRM state or Nucleus history. A
binary that cannot read the retained schema is not a safe rollback and
requires the database recovery plan for that release.

CRM carries the exact `Semantics-Project: crm` participation marker. Register
the canonical folder only after the implementation and marker exist, at the
current Decisions watermark. Seed revision zero only from already-authoritative
implemented terminology, verify HEAD, and remove any temporary seed input when
it is no longer needed. Registration is a separate operational effect, not
part of deployment.

## Coordinated deployment maintenance

```sh
crm --json maintenance hold RUN_ID
crm --json maintenance status
crm --json maintenance drain
crm --json maintenance release RUN_ID
crm --json maintenance canary --directory /absolute/private/canary-directory
```

The selected database parent owns `deployment-maintenance/`; the standard
location is `~/Library/Application Support/CRM/deployment-maintenance/`.
Databases in the same parent share this gate. This directory stores only
operational hold/lock metadata, never retained case or profile text.

Holds are durable and independently owned. New case/profile mutations, tell,
retry, and ordinary initialization/migration are rejected while held.
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

The coordinator composes the existing program installer and separate
`migrate --backup` command. Migration with `CELL_DEPLOYMENT_RUN_ID` acquires
exclusive activity only under its sole matching hold; an arbitrary environment
value never bypasses another owner or active work. The program installer still
never opens or migrates CRM data. `doctor` can validate held Nucleus readiness
for that exact deployment owner; normal steward admission remains strict.

The canary creates or resumes one product-marked private synthetic database,
uses the actual steward, and verifies one committed second revision, a visible
advisory, acknowledged tool result, and the exact correlated
`crm/case-steward/1` job. Deployment verification separately requires a completed
job and matching completed current attempt with nonblank thread, turn, and final
message output. A committed revision still remains ordinary CRM success after a
later runtime failure; that outcome alone does not verify a healthy deployment.
Rechecking a repaired Nucleus output read reuses the existing canary job and
preserves any CRM diagnostic recorded before the repair.
It returns `data.canary` with `protocol_version`, `verified`, database, case,
update, and job identities. Existing foreign directories are refused; failed
or uncertain work is retained and never automatically retried. It affects no
production case and sends no contact. Restore Nucleus admission after its own
verification before running requester canaries; production CRM holds remain.

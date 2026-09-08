# macOS user installation

CRM installs a short-lived CLI and its Chancery provider. It has no daemon,
LaunchAgent, scheduler, crawler, contact sender or direct Codex runner. CRM
launches a hidden worker only for queued or resumed updates. The worker uses
the separately installed Nucleus service.

Build and validate first:

```sh
./crm/ci.sh
<TESTED_CRM_INSTALL> install \
  --binary <TESTED_CRM_BINARY> \
  --bundle /Users/joey/rust/cell/crm/chancery
```

Deployment publishes stable command and provider selectors through one
content-addressed current-release selector. It validates binary/provider
version, exact release tree, content manifest, selector ownership, installed
help/version, and pre-commit rollback. It does not initialize, open, migrate,
back up, inspect, or delete CRM state and does not restart Nucleus or launch a
worker.

A PID-aware product lock serializes CRM updates. Provider publication also
takes the shared Chancery catalog-writer lock, always after the product lock,
so shared Rust installers cannot publish concurrently; stale lock
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
live workers and unsettled active updates. See the [data model](data-model.md#initialization-and-migration)
for transaction, backup, and database rollback semantics. A program downgrade
to a schema-one release requires restoring the compatible backup separately;
selector rollback does not downgrade the database.

New releases use the shared Rust `cell-install-v2` manifest in `manifest.json`.
The exact inventory includes `crm-install` at `bin/crm-install` and
`package/install`, the product binary, and `share/chancery/crm`.
`~/.local/bin/crm-install` follows the selected release. The Rust installer
also verifies the supported legacy format when admitting an existing install or
recovering a retained release.

## Paths

```text
~/.local/bin/crm
~/Library/Application Support/CRM/crm.db
~/Library/Application Support/CRM/install/{current,previous,releases/}
~/Library/Application Support/Chancery/providers/crm
```

The database and SQLite sidecars are retained independently from installed
releases. CRM has no uninstaller or automatic pruning. Removing cases,
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
secure database/sidecar permissions, and strict Nucleus/toolset readiness.

## Rollback

If deployment fails before commit, the deployer restores both binary and
provider views. After a committed update, preserve diagnostic evidence. Resolve
`install/previous` to its canonical owned release directory, then use a trusted
tested Rust installer to verify and select that retained release:

```sh
<TRUSTED_CRM_INSTALL> recover \
  --release <VERIFIED_PREVIOUS_RELEASE_DIRECTORY>
```

Do not execute an unverified installer from the retained release.
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

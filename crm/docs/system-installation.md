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

## Paths

```text
~/.local/bin/crm
~/Library/Application Support/CRM/crm.db
~/Library/Application Support/CRM/install/{current,previous,releases/}
~/Library/Application Support/Chancery/providers/crm
```

The database and SQLite sidecars are retained independently from installed
releases. Version 0.1 has no uninstaller or automatic pruning. Removing cases,
intake, steward updates, tool receipts, or retained releases is a separate
destructive action requiring explicit authority.

## Verify

```sh
/Users/joey/.local/bin/crm --version
/Users/joey/.local/bin/crm doctor
/Users/joey/.local/bin/chancery show crm.case.maintain
/Users/joey/.local/bin/chancery show crm.library.explore
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

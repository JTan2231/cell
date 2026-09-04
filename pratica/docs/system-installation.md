# macOS user installation

Pratica installs one short-lived CLI and one product-owned Chancery provider.
It has no daemon, LaunchAgent, scheduled work, source crawler, or direct Codex
runner. Commands that require judgment synchronously use the separately
installed Nucleus service.

Build and validate first:

```sh
./pratica/ci.sh
cargo build --release --locked --package pratica
pratica/packaging/macos/deploy-user.sh \
  --binary /Users/joey/rust/cell/target/release/pratica
```

Deployment publishes stable command and provider selectors through one
content-addressed current-release selector. It validates the binary/provider
version, exact release tree, content manifest, selector ownership, installed
help/version, and pre-commit rollback. It does not initialize, open, migrate,
back up, inspect, or delete negotiation state and does not restart Nucleus.

Initialize separately after a fresh deployment:

```sh
/Users/joey/.local/bin/pratica init
/Users/joey/.local/bin/pratica doctor
```

An existing schema-one database requires a separate, explicit quiescent
migration after the new release is selected:

```sh
/Users/joey/.local/bin/pratica migrate \
  --backup /absolute/private/path/pratica-schema-1.db
/Users/joey/.local/bin/pratica doctor
```

Stop every other Pratica process first and ensure no attempt is active. The
backup path must be absolute and absent inside an existing private directory;
Pratica creates the backup mode 0600 before changing schema one to schema two.
Running `migrate` on schema two is a true no-op and neither inspects nor creates
the named backup. Other source schema versions are refused.

## Paths

```text
~/.local/bin/pratica
~/Library/Application Support/Pratica/pratica.db
~/Library/Application Support/Pratica/install/{current,previous,releases/}
~/Library/Application Support/Chancery/providers/pratica
```

The database and SQLite sidecars are retained independently from installed
releases. Version 0.1 has no uninstaller or automatic pruning. Removing retained
contracts, reviews, attempts, or releases is a separate destructive action
requiring explicit authority.

## Verify

```sh
/Users/joey/.local/bin/pratica --version
/Users/joey/.local/bin/pratica doctor
/Users/joey/.local/bin/chancery show pratica.integration.negotiate
/Users/joey/.local/bin/chancery show pratica.agreement.explore
/Users/joey/.local/bin/chancery doctor
/Users/joey/.local/bin/nucleus health
```

Doctor proves Pratica storage and strict Nucleus/toolset readiness. It does not
prove a target system, steward source, negotiation, agreement, or conformance
result.

## CRM acceptance canary

The version-0.1 requester canary uses an isolated Pratica database and the
recent “Review CRM data model concerns” task as entrant evidence. It must:

1. capture only bounded source snapshots through their owning public
   Conversations route, without putting transcript bodies in fixtures, logs,
   release bytes, or Chancery;
2. register the relevant logical steward scopes on explicit versioned bases;
3. open one CRM integration and one bilateral track per actual system of
   concern, with complete entrant Markdown expectations;
4. obtain steward responses through `pratica/steward-response/1`, explicitly
   negotiate any counterproposals or blocks, and seal only exact mutually
   assented terms;
5. run an independent `pratica/composition-review/1` review across the sealed
   agreements and resolve or retain every reported contradiction or coverage
   gap visibly;
6. export the final exact Markdown agreement set and prove its offer digests,
   party assents, steward bases, Nucleus correlations, and Pratica seals; and
7. create no CRM database, source tree, migration, API, UI, deployment, release,
   or other implementation artifact.

A completed Nucleus job, polished review prose, or unsealed offer is not a
successful canary. Success is the Pratica-owned sealed contract set plus its
durable composition review. Conformance review is intentionally absent until a
real candidate implementation basis exists.

Use synthetic fixtures for CI. The real CRM terms remain private output of the
explicit canary and are never staged into the installed release.

## Rollback

If deployment fails before commit, the deployer restores both the binary and
provider views. After a committed update, preserve canary evidence and redeploy
the exact previous binary with its packaged deployer:

```sh
pratica_previous="/Users/joey/Library/Application Support/Pratica/install/previous"
"$pratica_previous/package/deploy-user.sh" \
  --binary "$pratica_previous/bin/pratica"
```

Normal selector, exact-tree, manifest, component-hash, and version checks still
apply. Stop if `previous` is absent or invalid. Rollback changes program and
provider selection only; it never rewrites negotiation state. A binary that
cannot read the retained schema is not a safe rollback and requires the
database recovery plan for that release.

After a schema-one to schema-two migration, rollback requires restoring the
retained schema-one backup and selecting the matching old binary together.
Redeploying only the old program against schema-two state is unsafe. Preserve
the newer database separately until the rollback decision and recovery
evidence are complete.

Pratica participates in Semantics as project `pratica`. Register the canonical
folder only after this marker and implementation exist, seed only implemented
terminology at revision zero, verify HEAD, and remove any temporary seed source
when it is no longer needed.

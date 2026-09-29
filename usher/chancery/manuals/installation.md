# Usher installation and release selection

This feature defines the installation guarantees of the separate Rust
`usher-install` executable. Use `usher.install.operate` for the installation
and recovery procedure. Recognition remains a read-only library and CLI.

## Interfaces and effects

The supported installer interfaces are:

```sh
usher-install install --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE \
  [--home ABSOLUTE_HOME] [--expected-current absent|releases/HASH]
usher-install inspect [--home ABSOLUTE_HOME]
usher-install verify --binary ABSOLUTE_BINARY --bundle ABSOLUTE_BUNDLE \
  [--home ABSOLUTE_HOME]
usher-install verify-release ABSOLUTE_RELEASE_DIRECTORY
ABSOLUTE_RETAINED_RELEASE/package/install recover \
  --release ABSOLUTE_RELEASE_DIRECTORY \
  [--home ABSOLUTE_HOME] [--expected-current absent|releases/HASH]
```

Installation and recovery change only Usher-owned local releases and selectors.
They create no database, semantic project, worker, schedule, credentials, or
other product state. They make no network or model call and require no Chancery
executable. Inspection and verification are read-only.

The default installation is under the current user's
`Library/Application Support/Usher/install`. `--home` explicitly selects an
isolated or alternate user's home. The public selectors are `.local/bin/usher`,
`.local/bin/usher-install`, and
`Library/Application Support/Chancery/providers/usher` under that home.

## Exact identity and consistency

The recognition binary, executing installer, and provider release must have
matching versions. Each new release retains `bin/usher`, `bin/usher-install`,
the exact recovery executable `package/install`, and `share/chancery/usher`.
The complete provider bundle, including its indexed overview, feature pages,
and procedures, is part of the immutable release.

`manifest.json` uses format `cell-install-v1`. It records product/provider
versions, retained file paths, SHA-256 hashes, modes, and the content-addressed
release ID. A version string alone proves neither integrity nor ownership.
Release identity, product version, provider schema, entry contract versions,
recognition JSON schemas, and adapter protocol are separate boundaries.

The installer verifies the prior release and takes the product lock before
the shared Chancery writer lock. One atomic `current` selector advances the
owned commands and provider together. An explicit `--expected-current` requires
that selection. When omitted, the installer captures the current selection
before waiting for the product lock. Stale or foreign selections are refused.

`inspect` reads the installation's local paths, versions, and integrity
metadata. `verify` compares the installed selection with the exact recognition
binary, provider bundle, and executing installer. `verify-release` checks one
retained release's integrity; it establishes neither that the release is current
nor authority to select it. No inspection returns recognition document bodies.

## Failure and recovery

A failed publication restores the captured prior selectors when their basis
can be verified. Changed bytes, foreign ownership, stale selection, and an
unprovable prior release stop the operation. An existing directory or matching
version is insufficient. Never edit immutable release bytes or manifests.

Deliberate recovery uses the retained exact Rust `package/install` executable
and an exact supported release under the selected home's
`Library/Application Support/Usher/install/releases`. It verifies ownership,
integrity, and the expected current selection before restoring selectors.
It does not rebuild a release. Direct installation and recovery retain releases
and provide no release-deletion operation.

Rust recovery also supports legacy `manifest.txt` releases from Usher's generated
shell installer. A selected legacy release has no installer binary; recovery
detaches the owned public `usher-install` selector. Inspection through a
retained Rust `package/install inspect` accepts that absence. Keep the retained
Rust executable so it can later restore a Rust release and its installer selector.

The legacy `package/deploy-user.sh` is archived evidence. It cannot recover
from a new-format current release because it does not understand `cell-install-v1`.
Use retained Rust recovery to select either supported format.

## Coordinated deployment

The Cell coordinator prepares committed production artifacts through the shared
release builder or reuses a matching sealed bundle. It verifies versions,
hashes, and source identity. Preparation records build evidence, not a CI claim.
The sealed `bin/usher-install adapter OP` implements the version-one JSON
adapter for inspection, installation, verification, and recovery.

Usher is stateless and creates no domain maintenance state. Installation
does not prove membership of any checkout. The coordinator's release cleanup
policy is separate from direct installer retention. Git publication, release
deletion, and other product operations require their own authority.

## Privacy and limits

Installation retains local executable and documentation releases and integrity
manifests. It retains no credentials or domain documents. Local paths and
release metadata remain caller-owned disclosure. No installation duration,
retention horizon, or release cadence is promised.

After installation, `usher --register-usage` registers command inventory without
recognition work. With a nonempty `CODEX_THREAD_ID`, Chancery's private usage
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal calls are excluded. Recording errors preserve results.

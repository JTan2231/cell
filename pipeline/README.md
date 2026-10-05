# Cell product descriptors and releases

This directory contains the literal product descriptors, wrapper generator,
and manual release command. [Telete](../infrastructure/telete/README.md) owns
queued CI, validation, repair, production preparation, deployment, and outcome
notification.

Commit a change and submit it from the Cell root:

```sh
./ci.sh submit COMMIT
./ci.sh status JOB
```

New jobs skip tests by default. Add `--run-tests` to run selected tests. Prepare
the pinned runner with `telete prepare-tools` before jobs that run tests. Read
[Telete's operating contract](../infrastructure/telete/chancery/manuals/queue-operate.md)
for selection, signing, retained evidence, and recovery. Root and product
`ci.sh` wrappers and `pipeline/test.sh` select installed Telete.

## Maintain descriptors and wrappers

`products/*.sh` declares each product's identity, source root, Cargo packages,
provider bundles, native executables, checks, and independently versioned
release units. Telete and Usher read these literal assignments. Source groups
do not determine deployment scope or instruction order.

Regenerate entry points after changing their shared form or a product path:

```sh
pipeline/generate.sh --write
pipeline/generate.sh --check
```

Repeat `--product PRODUCT` to select descriptors. Generated wrappers locate the
Cell root from the descriptor's repository-relative `PRODUCT_DIR`.

## Publish a product release

Product `release.sh` wrappers invoke `pipeline/release.sh`. This command updates
the selected release unit's version, commits, tags, and pushes atomically. It
requires publication authority, a clean `main`, a configured `origin`, and
matching remote history. It is not a build-only or CI command.

The release command uses the caller's Cargo environment for offline metadata
reads and lockfile updates. Required dependencies must already be available.
It publishes source without building, signing, or deploying it. Submit the
published commit through CI for validation, production preparation, and
deployment:

```sh
./ci.sh submit HEAD --deploy PRODUCT
```

The published tag records source publication. It does not prove validation,
build, or signing success. Inspect the retained Telete job outcome before
treating the release as ready. Telete is the sole product deployment route.

`RELEASE_COMPANION_MANIFESTS` lists `release-unit|package-manifest` rows for
provider-owned libraries released at their owner's version. The release
command requires matching versions and updates each companion with the primary
manifest, root lockfile, and provider bundles. Companions have no separate tag.

The release command holds a lock in Git's common directory and rechecks remote
`main` and tag absence before publication. macOS uses `shlock`; the portable
`mkdir` fallback fails closed. Confirm that no release is active before
removing a stale `cell-release-publication.lock.d`.

Read [Telete queue operation](../infrastructure/telete/chancery/manuals/queue-operate.md)
for candidate preparation, signing, external storage, and deployment recovery.

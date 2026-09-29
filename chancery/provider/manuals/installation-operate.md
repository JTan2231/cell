# Install and recover the Chancery reader

Use this operation to select a tested current-user macOS Chancery release or
recover a retained release. Chancery has no service or scheduled process.
Installation changes its program, installer, and documentation selectors. It
preserves other products' provider selectors and the separate usage journal.
Nucleus health and authentication are not prerequisites.

Read `chancery resolve chancery.installation.operate` for required bundle,
catalog, and usage features. The procedure does not authorize another product's
release or provider selector, executing a represented capability, or changing
usage history. Use the owning product's installation route for its bundle.

## Select and verify the candidate

1. Identify the tested Chancery binary, matching tested installer, and reviewed
   Chancery bundle as absolute paths. Confirm the intended release and user
   installation. Use the owning release process; a source checkout does not
   prove a tested candidate.
2. Validate the source bundle with the compatible candidate reader:

   ```sh
   <TESTED_CHANCERY_BINARY> validate /Users/joey/rust/cell/chancery/provider
   ```

3. Inspect the current selection with a trusted tested installer:

   ```sh
   <TESTED_CHANCERY_INSTALL> inspect
   ```

4. Preserve the prior selection and verify that any existing Chancery-owned
   selectors are owned targets. Stop on a foreign selector, changed retained
   release, invalid bundle, or unsupported installation. Never silently take
   over another provider's selector.
5. Verify the candidate without selecting it:

   ```sh
   <TESTED_CHANCERY_INSTALL> verify --binary <TESTED_CHANCERY_BINARY> \
     --bundle /Users/joey/rust/cell/chancery/provider
   ```

`--home ABSOLUTE_HOME` selects an explicit user-home installation for installer
inspection, verification, installation, or recovery. With no override, use the
current user's home. An optional
`--expected-current absent|releases/HASH` guards installation and recovery
against a changed current selection. Use the exact captured selection; do not
invent an expected digest.

## Install and prove the selection

1. Install the tested candidate:

   ```sh
   <TESTED_CHANCERY_INSTALL> install --binary <TESTED_CHANCERY_BINARY> \
     --bundle /Users/joey/rust/cell/chancery/provider
   ```

2. Confirm the installed version and inspect the selected release. The deployer
   stages a content-addressed release and switches `current`, `previous`, the
   command and installer selectors, and `providers/chancery` with rollback
   support. It then verifies the installed command. No product work runs.
3. Register the selected program's declared command inventory:

   ```sh
   /Users/joey/.local/bin/chancery --register-usage
   ```

   Registration is a separate installation step. It initializes only an empty
   supported Chancery journal and preserves existing history. Stop and follow
   `chancery.usage.operate` if that journal is unsupported; installation does
   not authorize recreating it.
4. Read installed documentation and compatibility:

   ```sh
   /Users/joey/.local/bin/chancery doctor
   /Users/joey/.local/bin/chancery product chancery
   /Users/joey/.local/bin/chancery list --provider chancery
   /Users/joey/.local/bin/chancery show chancery.directory.discover
   /Users/joey/.local/bin/chancery resolve chancery.directory.discover
   ```

5. Confirm that the overview and inventory come from the selected release and
   that other provider selectors remain present. A doctor issue can belong to
   another provider. Preserve that issue and use its owning repair route; an
   installed reader does not make a represented service ready.

The installed layout is:

```text
~/.local/bin/chancery -> current Chancery release
~/.local/bin/chancery-install -> current Chancery installer
~/Library/Application Support/Chancery/
  providers/                 product-owned provider selectors
  usage.sqlite3              separate private usage journal, when initialized
  install/
    releases/RELEASE_ID/
      bin/chancery
      bin/chancery-install
      package/install
      share/chancery/chancery/ Chancery-owned provider bundle
      manifest.json           cell-install-v2 exact inventory
    current -> releases/RELEASE_ID
    previous -> releases/RELEASE_ID
```

Chancery owns only `providers/chancery`, which follows its current release.
The installer verifies supported legacy layout when admitting or recovering
an older release. Bundle publication guarantees and schema compatibility are
owned by `chancery.bundle.validate`.

## Recover a retained release

1. Identify the canonical owned release directory resolved from
   `install/previous`. Preserve the failed selection and diagnostics. Use a
   trusted tested installer; do not execute an unverified installer found
   inside the retained release.
2. Validate that retained release:

   ```sh
   <TESTED_CHANCERY_INSTALL> verify-release ABSOLUTE_RELEASE_DIRECTORY
   ```

3. Recover the verified selection:

   ```sh
   <TESTED_CHANCERY_INSTALL> recover --release ABSOLUTE_RELEASE_DIRECTORY
   ```

4. Inspect the selection, run the installed version and documentation reads,
   and confirm coherent program and bundle selection. Recovery changes
   Chancery's program and documentation only. It does not erase usage rows or
   repair other products.

Stop if integrity checks fail, selector ownership is foreign, or a supported
retained release cannot be identified. Do not edit installed immutable files.
For an invalid product bundle, validate and redeploy that product through its
own release procedure. Removing the reader must preserve product selectors
so a compatible reader can read them later. This operation supplies no catalog
cleanup or journal-deletion procedure.

## Privacy and completion

Install only public documentation with no secrets or transient private output.
Current-user filesystem ownership protects the installation and private journal.
Catalog reads retain no snapshot; agent dispatch can append only command
identity, time, and thread metadata. Recording errors preserve command results.

Completion requires the verified intended program and matching bundle, retained
other provider selectors, and explicit post-install registration outcome.
Catalog availability and compatible documentation are separate from readiness
and domain success of any represented product.

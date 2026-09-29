# Install or recover Usher

Use this procedure for an authorized installation, update, inspection, or
retained-release recovery. Read `usher.installation` for release identity,
selector, integrity, consistency, and legacy compatibility guarantees. Use
`chancery resolve usher.install.operate` to include that required contract.

## Establish the candidate and authority

1. Select ordinary CI delivery or a separately authorized manual installation.
   Submit source changes with `cell-ci submit COMMIT` or `./ci.sh submit COMMIT`
   from Cell. The manager integrates, validates, attempts bounded repairs,
   deploys, and emails the outcome. Verify its retained outcome.
2. Select a validated source candidate for manual installation. Preparation
   does not rerun validation or require a CI receipt. Build both production
   executables through the shared release builder:

```sh
python3 /absolute/cell/deployment/build.py --source-root /absolute/cell \
  --product usher --output /absolute/cell-build
```

3. Establish matching recognition-binary, executing-installer, and provider
   versions. Inspect the intended prior selection. Use `--home ABSOLUTE_PATH`
   only for an intentional isolated or alternate-user installation.

Stop when ownership, candidate integrity, or the prior release cannot be proved.
Do not adopt a foreign selector or edit a retained release.

## Install and verify

1. Install the exact candidate. Add `--expected-current absent|releases/HASH`
   when installation must require a specific prior selection:

```sh
/absolute/cell-build/candidates/usher/bin/usher-install install \
  --binary /absolute/cell-build/candidates/usher/bin/usher \
  --bundle /absolute/cell/usher/chancery
```

2. Verify installed identity and coherent command/provider selection:

```sh
usher-install inspect
/absolute/cell-build/candidates/usher/bin/usher-install verify \
  --binary /absolute/cell-build/candidates/usher/bin/usher \
  --bundle /absolute/cell/usher/chancery
usher-install verify-release /absolute/Usher/install/releases/HASH
```

3. Read `chancery product usher`, `chancery show usher.recognition.inspect`,
   and `chancery resolve usher.install.operate` to verify the installed
   overview, feature pages, and required contract reading. Run
   `usher --register-usage` to register its command inventory.

Installation changes Usher-owned release files and selectors. It creates no
semantic project, database, worker, schedule, or other product state. It does
not assess checkout membership. Inspection and verification are read-only;
version output alone is not integrity proof. No Chancery executable is required
to install Usher; omit catalog checks when that reader is absent.

For a coordinated manual deployment, use `./deploy.sh usher` from Cell. Its
sealed Rust adapter owns Usher installation and recovery. The coordinator's
cleanup policy is separate from direct installer retention.

## Recover an exact retained release

1. Stop after a failed publication if the recorded prior selection cannot be
   verified. Preserve the failure evidence; do not overwrite selectors to
   bypass foreign ownership, changed bytes, or a stale selection.
2. Select the exact supported retained release under the intended home's
   `Library/Application Support/Usher/install/releases`. Keep a new-format
   release's exact Rust `package/install` executable available.
3. Recover with that executable and the intended current-selection precondition:

```sh
/absolute/retained-release/package/install recover \
  --release /absolute/Usher/install/releases/HASH \
  --expected-current releases/CURRENT_HASH
```

4. Inspect and verify the recovered release before declaring completion.

Recovery accepts `--home` and the same `--expected-current` forms as install.
Selecting a legacy `manifest.txt` release detaches the owned public
`usher-install` selector. Inspect through the retained Rust `package/install`
and keep it for later recovery to a Rust release. The legacy shell deployer
cannot recover from a new-format current release.

This procedure does not authorize Git publication, retained-release deletion,
foreign-selector takeover, semantic registration, or other product operations.
Inspection may disclose local paths and integrity metadata. Keep that output
within the intended local boundary.

## Command usage

With a nonempty `CODEX_THREAD_ID`, Chancery's private usage journal records
command identity, time, and thread ID, not arguments, output, or outcomes.
Internal calls are excluded. Recording errors preserve command results.

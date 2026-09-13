# Install or recover Usher

Complete relevant development checks with `./ci.sh usher` before release or
deployment. Preparation assumes those checks have passed without rerunning CI
or requiring a CI receipt. Build both production executables with the shared
release builder. Installation uses the separate Rust `usher-install` executable
and shared `cell-install` library:

```sh
python3 /absolute/cell/deployment/build.py --source-root /absolute/cell \
  --product usher --output /absolute/cell-build
/absolute/cell-build/candidates/usher/bin/usher-install install \
  --binary /absolute/cell-build/candidates/usher/bin/usher \
  --bundle /absolute/cell/usher/chancery
```

The recognition binary, executing installer and provider release must have
matching versions. The installer stages immutable bytes, verifies the exact
prior release, takes the product lock before the shared Chancery writer lock,
and advances the owned commands and provider through one atomic `current`
selector. No database, semantic project, worker, schedule, or other product is
changed. No Chancery executable is required. The `usher` recognition library
and CLI remain read-only and do not gain installation operations.

Use `--expected-current absent|releases/HASH` to require a specific prior
selection. If you omit it, the installer records the current selection before
waiting for the product lock. Use `--home ABSOLUTE_PATH` for an isolated home or
another user's home. The default installation is under the current user's
`Library/Application Support/Usher/install`. The public selectors are
`.local/bin/usher`, `.local/bin/usher-install`, and
`Library/Application Support/Chancery/providers/usher`.

## Release identity and verification

Each new release retains `bin/usher`, `bin/usher-install`, the exact recovery
executable `package/install`, and `share/chancery/usher`. `manifest.json` uses
format `cell-install-v1` and records product/provider versions, retained file
paths with SHA-256 hashes and modes, and the content-addressed release ID.
Version output alone is not an integrity or ownership proof. Never edit an
immutable release or adopt a foreign command or provider selector.

Read the installation and verify it against the exact candidate:

```sh
usher-install inspect
/absolute/cell-build/candidates/usher/bin/usher-install verify \
  --binary /absolute/cell-build/candidates/usher/bin/usher \
  --bundle /absolute/cell/usher/chancery
usher-install verify-release /absolute/Usher/install/releases/HASH
```

`inspect` and `verify` accept the same optional `--home ABSOLUTE_PATH`.
`verify` checks the installed candidate using the executing installer as part
of its exact identity. `verify-release` checks one retained release's integrity
without changing selectors; it does not establish that the release is current
or grant permission to select it. Inspect and verification expose local paths,
versions and integrity metadata, not recognition document bodies.

## Failure and deliberate recovery

If publication fails, the installer restores the recorded prior selectors when
it can verify them. Stop if ownership is foreign or unverified, bytes were
changed, or the selection is stale. A matching version or existing directory
does not permit recovery. The installer retains releases and does not delete them.

For a new-format retained release, use its exact packaged recovery executable
and the exact intended release directory:

```sh
/absolute/retained-release/package/install recover \
  --release /absolute/Usher/install/releases/HASH \
  --expected-current releases/CURRENT_HASH
```

Recovery accepts `--home ABSOLUTE_PATH` and the same `--expected-current`
precondition as installation. The target must be a retained release under that
home's `Library/Application Support/Usher/install/releases`. Recovery verifies
the supported immutable release and ownership before restoring selectors; it
does not rebuild a release or edit its manifest.

The Rust recovery command also supports the legacy `manifest.txt` format from
Usher's generated shell installer. Restoring a legacy release detaches the owned
`.local/bin/usher-install` selector because that release has no installer
binary. Inspection through a retained Rust `package/install inspect` accepts
that absence while the legacy release is current. Keep the new-format release's
exact `package/install`: invoking it directly can later recover that Rust
release and restore the public installer selector.

The legacy release's `package/deploy-user.sh` remains archived release evidence.
It does not understand `cell-install-v1` and cannot perform recovery from a
new-format current release. Use the retained Rust `package/install recover`
for the switch to either supported format. New Usher installations have no
checked-in shell installer or Python Usher deployment adapter.

## Coordinated deployment

From a Cell checkout, `./deploy.sh usher` prepares the committed candidate
through the shared release builder. It builds only production binaries or reuses
a matching sealed bundle, verifies versions, hashes and source identity, and
records build evidence without making a CI claim. The Python coordinator invokes
the sealed `bin/usher-install adapter OP` under the existing version-one JSON protocol.
The Rust adapter owns Usher's inspection, installation, verification and
recovery. As a stateless product, it creates no domain maintenance state.

Installation does not prove membership of any checkout. Release publication,
retained-release deletion and other product operations require their own
authority. The coordinated Cell command has its separately documented cleanup
policy; direct Usher installation and recovery retain their release history.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.

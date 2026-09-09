# macOS system installation

Build and test the package and Rust installer through the Cell product gate.
Then install those exact tested artifacts:

```sh
./ci.sh conversations
<TESTED_CONVERSATIONS_INSTALL> install \
  --binary <TESTED_CONVERSATIONS_BINARY> \
  --bundle /Users/joey/rust/cell/conversations/chancery
```

The deployment has no service to start and no credential to source. Each CLI
invocation owns and cleans up its private App Server process group, including
descendants created by a Codex wrapper; it does not inspect or terminate other
Codex processes. The deployer creates:

- `~/.local/bin/conversations` and `~/.local/bin/conversations-install` as the
  stable command and installer selectors;
- `~/Library/Application Support/Conversations/install/releases/HASH` as the
  immutable, content-addressed release;
- `install/current` and `install/previous` selectors; and
- `~/Library/Application Support/Chancery/providers/conversations` selecting
  the current release's provider bundle.

Krisis and Paperboy embed the Conversations library. Rebuild and deploy each
consumer to apply normalization changes such as accepting empty message text;
replacing the Conversations CLI alone does not update embedded callers.

The `cell-install-v2` release identity covers the binary, Rust installer, public
layout, and complete Chancery bundle. `manifest.json` records the exact inventory;
`package/install` retains the installer bytes.
An identical deployment is a no-op. Existing release bytes are verified before
reuse, and any selected current or previous release must have an exact
content-addressed selector plus a self-consistent manifest and component
hashes. A PID-aware product lock serializes Conversations updates, and a shared
Chancery catalog-writer lock serializes provider publication with the other
shared Rust installers. The installer takes the product lock before the catalog
lock and recovers stale owners. It publishes `current` atomically. If a version
or help check fails after the switch, it restores the previous selectors. If
it cannot verify restoration, it detaches them. The installer
refuses to replace a foreign selector, trust a malformed or tampered selected
release, or accept a provider selector without a current release.

By default deployment snapshots `current` before waiting for the product lock
and rejects a stale cutover. Concurrent callers may make that guard explicit
with `--expected-current absent` for a fresh install or
`--expected-current releases/HASH` for an update.

Deployment does not run `doctor`, scan Codex metadata, copy transcripts, or
alter Codex authentication. Run `conversations doctor` separately under the
operator's normal environment. To roll back, resolve `install/previous` to its
canonical owned release directory, then run a trusted tested
`conversations-install recover --release ABSOLUTE_RELEASE_DIRECTORY`. The Rust
installer verifies the complete retained release before selecting it; it never
executes a retained release's installer as a verification step. Recovery supports
the previous shell-installed format as well as `cell-install-v2`.

# macOS system installation

Build the independently versioned package from the Cell workspace, then run
the user deployer explicitly:

```sh
cargo build --release --package conversations
conversations/packaging/macos/deploy-user.sh \
  --binary /absolute/path/to/target/release/conversations
```

The deployment has no service to start and no credential to source. Each CLI
invocation owns and cleans up its private App Server process group, including
descendants created by a Codex wrapper; it does not inspect or terminate other
Codex processes. The deployer creates:

- `~/.local/bin/conversations` as the stable command selector;
- `~/Library/Application Support/Conversations/install/releases/HASH` as the
  immutable, content-addressed release;
- `install/current` and `install/previous` selectors; and
- `~/Library/Application Support/Chancery/providers/conversations` selecting
  the current release's provider bundle.

The release identity covers the binary, deployer, and complete Chancery bundle.
An identical deployment is a no-op. Existing release bytes are verified before
reuse, and any selected current or previous release must have an exact
content-addressed selector plus a self-consistent manifest and component
hashes. A PID-aware product lock serializes Conversations updates, and a shared
Chancery catalog-writer lock serializes provider publication with the other
generated selector-only deployers. Locks are taken product first and catalog
second; stale owners are recovered. The `current` selector is published
atomically, and a failed post-switch version or help smoke restores the prior
selector view or detaches it if restoration cannot be proved. The installer
refuses to replace a foreign selector, trust a malformed or tampered selected
release, or accept a provider selector without a current release.

By default deployment snapshots `current` before waiting for the product lock
and rejects a stale cutover. Concurrent callers may make that guard explicit
with `--expected-current absent` for a fresh install or
`--expected-current releases/HASH` for an update.

Deployment does not run `doctor`, scan Codex metadata, copy transcripts, or
alter Codex authentication. Run `conversations doctor` separately under the
operator's normal environment. To roll back, redeploy the exact binary and
packaged deployer from `install/previous`; the content address will become the
current selector again after normal validation.

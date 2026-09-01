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
hashes. A per-product lock serializes updates, and a failed post-switch version
or help smoke restores all prior selectors. The installer refuses to replace a
foreign selector, trust a malformed or tampered selected release, or accept a
provider selector without a current release.

Deployment does not run `doctor`, scan Codex metadata, copy transcripts, or
alter Codex authentication. Run `conversations doctor` separately under the
operator's normal environment. To roll back, redeploy the exact binary and
packaged deployer from `install/previous`; the content address will become the
current selector again after normal validation.

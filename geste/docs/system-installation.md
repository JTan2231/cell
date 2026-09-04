# macOS user installation

Geste installs one short-lived CLI and one product-owned Chancery provider. It
has no daemon, LaunchAgent, Nucleus requester, network call, or automatic
ingestion.

Build and validate first:

```sh
./geste/ci.sh
cargo build --release --locked --package geste
geste/packaging/macos/deploy-user.sh \
  --binary /Users/joey/rust/cell/target/release/geste
```

Deployment publishes stable command and provider selectors through one
content-addressed `current` selector, then atomically switches that selector.
It validates the binary/provider version, exact release tree, content manifest,
selector ownership, installed help/version, and pre-commit rollback. It does
not initialize or migrate domain state.

A PID-aware product lock serializes Geste updates. Provider publication also
takes the shared Chancery catalog-writer lock, always after the product lock,
so generated selector-only deployers cannot publish concurrently; stale lock
owners are recovered. Deployment snapshots `current` before waiting and
rejects a stale cutover. Callers may instead supply
`--expected-current absent|releases/HASH`. The one `current` switch is atomic;
failed smoke checks restore the prior selector view or detach it if restoration
cannot be proved.

Initialize explicitly after a fresh deployment:

```sh
/Users/joey/.local/bin/geste init
/Users/joey/.local/bin/geste --json doctor
```

## Paths

```text
~/.local/bin/geste
~/Library/Application Support/Geste/geste.db
~/Library/Application Support/Geste/install/{current,previous,releases/}
~/Library/Application Support/Chancery/providers/geste
```

The database and sidecars are retained independently of installed releases.
Version 0.1 has no uninstaller or automatic pruning. Removing retained state is
a separate destructive action requiring explicit authority.

Initialization explicitly forces new database bytes to mode 0600 before
SQLite opens them; inherited umask cannot weaken or strand that state. Doctor
also requires the complete schema-one object set and refuses any committed
revision lacking its final seal.

## Verify

```sh
/Users/joey/.local/bin/geste --version
/Users/joey/.local/bin/geste --json doctor
/Users/joey/.local/bin/chancery show geste.episode.explore
/Users/joey/.local/bin/chancery show geste.episode.capture
/Users/joey/.local/bin/chancery doctor
```

A domain canary creates a real episode and then proves search, historical show,
report, and graph from the installed database. Use genuine source anchors and
do not fabricate a Decisions event to make a settlement appear verified. If a
new effectful turn has not yet been admitted by Decisions, retain a Todo for a
later self-episode rather than creating a misleading partial canary.

If an installed domain canary fails after deployment committed, preserve its
evidence and redeploy the exact previous binary with its packaged deployer:

```sh
geste_previous="/Users/joey/Library/Application Support/Geste/install/previous"
"$geste_previous/package/deploy-user.sh" \
  --binary "$geste_previous/bin/geste"
```

Normal selector, exact-tree, manifest, component-hash, and version checks still
apply. Stop if `previous` is absent or invalid; do not rewrite selectors by
hand. Rollback changes installed program/provider selection and leaves the
separate episode database untouched.

Geste participates in Semantics as project `geste`. Register the canonical
folder after its exact marker exists, seed only the implemented terminology at
revision zero, verify HEAD, and then remove the temporary seed source if no
longer needed.

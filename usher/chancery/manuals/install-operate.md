# Install or recover Usher

Build a green candidate with `./ci.sh usher`. Validate its product-owned
Chancery bundle, then deploy through the generated selector-only profile:

```sh
usher/packaging/macos/deploy-user.sh --binary /absolute/target/release/usher
```

The candidate version must match the provider release. The deployer stages
immutable bytes, verifies the exact prior release, takes the product lock
before the shared Chancery writer lock, and advances the owned command and
provider through one atomic current selector. No database, semantic project,
worker, schedule, or other product is changed. No Chancery executable is required.

Use `--expected-current absent|releases/HASH` to require an exact observed
selection; omission snapshots the current selection before waiting for the
product lock. `--home` selects only an intentional isolated or alternate-user
boundary. The default installation is under the current user's
`Library/Application Support/Usher/install`, with `.local/bin/usher` and
`Library/Application Support/Chancery/providers/usher` selectors.

Verify `usher --version`, `usher --help`, the release manifest and owned
selectors. A failed publication restores the prior selectors. Deliberate
recovery uses the exact retained previous release's `package/deploy-user.sh`
with that release's `bin/usher`; never edit immutable release bytes. Refuse
foreign, tampered, stale, or unprovable ownership. Installation does not prove
membership of any checkout. Release publication and retained-release deletion
are separate actions, not side effects of this operation.

# Install or initialize Geste

Geste is a user-owned macOS CLI with a private SQLite casebook. It has no
daemon, service, authentication, model, network, Chancery runtime call, or
other Cell-product runtime dependency.

Build and validate the candidate first. Deployment is a separate authorized
effect:

```sh
<TESTED_GESTE_INSTALL> install \
  --binary <TESTED_GESTE_BINARY> \
  --bundle /Users/joey/rust/cell/geste/chancery
```

The Rust installer requires a regular executable candidate and a complete
version-matched Chancery bundle, each at an absolute path. The binary's `geste VERSION` output must match
the provider and installer release exactly. It hashes the binary, Rust installer,
public layout, and complete
provider tree into one content-addressed release under
`$HOME/Library/Application Support/Geste/install/releases` and retains a
`cell-install-v2` manifest in `manifest.json`. The release retains the installer
at `bin/geste-install` and `package/install`; `~/.local/bin/geste-install` follows
current alongside the product command and provider.

The stable command and Geste's sole provider selector both pass through one
atomic `install/current` release selector:

```text
~/.local/bin/geste
  -> .../Geste/install/current/bin/geste

.../Chancery/providers/geste
  -> .../Geste/install/current/share/chancery/geste
```

On update, the one current-link replacement switches the binary and provider
view together; `previous` retains the prior valid release. Identical
redeployment is idempotent. Existing current and previous releases are accepted
only after selector form, directory shape, manifest, provider-version, and
content hashes are proved. Legacy and `cell-install-v2` release trees are exact;
unmanifested files or directories are refused. Symbolic candidates, traversal
selectors, fabricated or tampered releases, regular-file selectors, and
selectors aimed outside this installation are refused rather than adopted.

A PID-aware product lock serializes Geste updates. The deployer then takes the
shared Chancery catalog-writer lock before rechecking and publishing the
provider view. By default it snapshots `current` before waiting and rejects the
cutover if another deployment changed it. A planner may make that precondition
explicit with `--expected-current absent` or
`--expected-current releases/HASH`.

A failed installed version/help smoke restores the exact prior current and
previous selectors. If coherent restoration cannot be proved, the deployer
detaches Geste's public selectors and stops fail-closed. Inspect ownership and
the last known valid release before retrying; do not bypass refusal or overwrite
a foreign path.

If an installed program regression requires rollback after deployment committed,
retain the failure evidence and resolve `install/previous` to its canonical
owned release directory. Use a trusted tested `geste-install recover --release
ABSOLUTE_RELEASE_DIRECTORY` to verify and select it. Do not execute an unverified
installer from the retained release. Stop if `previous` is absent or
invalid; never rewrite the selectors manually. Program rollback does not
rewrite or delete the separately retained episode database.

Deployment intentionally never creates, opens, migrates, backs up, or deletes
the episode database. After a successful install, initialize persistent state
as a separate visible choice:

```sh
/Users/joey/.local/bin/geste init
/Users/joey/.local/bin/geste doctor
```

The database resolves from `--database`, then nonempty `GESTE_DATABASE`, then
`$HOME/Library/Application Support/Geste/geste.db`; it never defaults to the
current directory. `init` creates schema one and is idempotent only for an
existing supported Geste database. It refuses foreign or unsupported storage.
`doctor` checks only existence, schema one, `foreign_key_check`,
`integrity_check`, the complete required schema object set, revision seals, a
mode-0600 database, and its mode-0700 state directory. Initialization forces
new database bytes to 0600 independently of inherited umask. Doctor does not
check source products, Chancery, Semantics, Nucleus, a model, or the network.

Release bundles contain no episode data. The application-support database may
contain sensitive authored accounts and source identities. Do not copy, print,
prune, uninstall, or delete retained state without separate explicit authority.

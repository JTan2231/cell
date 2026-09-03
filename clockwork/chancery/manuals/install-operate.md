# Install or diagnose Clockwork

Clockwork installs one current-user CLI/short-lived broker and one
Clockwork-owned Chancery provider. Program deployment is intentionally
independent from product schedule migration: it does not create or open the
Clockwork runtime database, register a definition, switch or disable a binding,
write an `org.clockwork.*` product plist, or run a product job.

Build and validate the candidate first. Deployment is a separate authorized
effect:

```sh
clockwork/packaging/macos/deploy-user.sh \
  --binary /absolute/path/to/clockwork \
  --chancery /absolute/path/to/chancery
```

The deployer requires a regular executable candidate, regular product-owned
deployer and uninstaller, complete provider bundle, and a separately supplied
regular executable candidate Chancery reader. The candidate's `clockwork
VERSION` output must exactly match provider release. It hashes the binary, both
packaging scripts, and complete provider tree into one release under
`$HOME/Library/Application Support/Clockwork/install/releases`, stores a
canonical manifest, then asks that reader to validate the exact provider copy
inside the staged release before any public selector mutation. Before commit,
the same reader must discover all three Clockwork entries through the installed
providers registry and selected provider path.

Packaging may create missing current-user `.local/bin` and Chancery parent
directories. It validates but does not chmod an existing shared parent;
Clockwork changes modes only within its own installation/state tree.

Both stable public selectors pass through one atomic `install/current` release
selector:

```text
~/.local/bin/clockwork
  -> .../Clockwork/install/current/bin/clockwork

.../Chancery/providers/clockwork
  -> .../Clockwork/install/current/share/chancery/clockwork
```

An update validates existing owned selector form and retained releases before
staging. Identical deployment is idempotent. A changed deployment preserves a
validated prior selection as `previous`, stages the exact immutable release,
and atomically replaces `current`. Symbolic candidates, foreign stable paths,
selectors escaping the release tree, version mismatch, malformed manifests,
or changed release bytes are refused rather than adopted.

If an installed version/help smoke fails before commit, the deployer restores
the prior current and previous selectors and public command/provider views. If
that cannot be completed coherently, it detaches all four public selectors and
reports the fail-closed state while retaining releases. Inspect the reported
owned paths before retrying; do not replace a foreign path or bypass content
checks.

After a committed deployment, diagnose locally:

```sh
/Users/joey/.local/bin/clockwork --version
/Users/joey/.local/bin/clockwork doctor
/Users/joey/.local/bin/chancery show clockwork.schedule.operate
/Users/joey/.local/bin/chancery doctor
```

Doctor opens the schema-one local store, initializes only an empty unversioned
file, prepares private directories, runs SQLite `quick_check`, resolves the current executable and
`/bin/launchctl`, and may mark retained `running` activations `lost` after
proving their recorded broker and any child absent. It executes no product,
changes no binding, and proves no future timer or product-domain result.

If a later authorized product canary fails, preserve its evidence and separate
program failure from product-definition or domain failure. Roll program bytes
back only by redeploying the exact packaged previous candidate:

```sh
clockwork_previous="/Users/joey/Library/Application Support/Clockwork/install/previous"
"$clockwork_previous/package/deploy-user.sh" \
  --binary "$clockwork_previous/bin/clockwork" \
  --chancery /absolute/path/to/chancery
```

Normal content, version, and ownership checks still apply. Program rollback
does not restore or rewrite a product binding. Generated plists pin an exact
content-addressed installed Clockwork binary, so never prune a Clockwork release while a plist
or running activation may refer to it.

To detach the stable public selectors, first disable every binding through the
schedule contract and verify quiescence. Then:

```sh
clockwork/packaging/macos/uninstall-user.sh
```

The uninstaller refuses while any regular or symbolic
`~/Library/LaunchAgents/org.clockwork.*.plist` remains. It validates and
removes only Clockwork's owned `~/.local/bin/clockwork`,
`providers/clockwork`, `install/current`, and `install/previous` selectors. It
serializes with deployment through `/usr/bin/shlock` on the private product
installation lock, including its atomic live/stale PID decision. When the
Clockwork state root is absent, uninstall may create that empty private root to
use the shared lock path; it creates no runtime database. It does not
boot out or delete a schedule, kill an activation, delete releases,
open or remove the database, prune history, or touch product artifacts or
logs. Absence of a plist is not proof that a manual child is not already
running, so quiescence remains an operator precondition.

Runtime state under application support can reveal private product paths,
digests, schedules, and activation times. Release bundles contain none of it.
Retained-state deletion or release pruning is a separate destructive action
requiring exact targets and proof that no generated plist or activation refers
to the release.

# macOS user installation

Clockwork installs one short-lived CLI/broker and one product-owned Chancery
provider. Program deployment does not initialize its database, register a
product definition, create a binding, install a product LaunchAgent, or run a
product job.

Build and validate first, then deploy under separate authority:

```sh
./clockwork/ci.sh
<TESTED_CLOCKWORK_INSTALL> install \
  --binary <TESTED_CLOCKWORK_BINARY> \
  --bundle /Users/joey/rust/cell/clockwork/chancery \
  --chancery /absolute/path/to/chancery
```

Deployment stages the binary, Rust installer, and complete provider bundle in
one content-addressed release. It atomically selects that release for the stable
command and provider paths. Before either selector changes, the supplied
candidate Chancery reader must validate the provider copy in that staged release.
Before commit, the same reader must find all three Clockwork entries through
the installed provider registry and selected provider path. Deployment retains
the prior valid selector for rollback.
It neither calls `clockwork binding switch` nor scans another product for jobs.
Missing current-user `.local/bin` and Chancery parent directories may be
created; existing shared parents are validated without changing their modes.

New releases use the shared Rust `cell-install-v2` manifest in `manifest.json`.
The exact inventory includes `clockwork-install` at `bin/clockwork-install` and
`package/install`, the product binary, and `share/chancery/clockwork`.
`~/.local/bin/clockwork-install` follows the selected release. The Rust installer
also verifies the supported legacy format when admitting an existing install or
recovering a retained release.

## Installed paths

```text
~/.local/bin/clockwork
~/Library/Application Support/Clockwork/clockwork.db
~/Library/Application Support/Clockwork/locks/
~/Library/Application Support/Clockwork/install/{current,previous,releases/}
~/Library/Application Support/Chancery/providers/clockwork
~/Library/LaunchAgents/org.clockwork.*.plist
~/Library/Logs/Clockwork/
```

The database, locks, logs, product LaunchAgents, and product release artifacts
are retained independently of the installed public selector. They are never
packaged into a Clockwork release.

## Verification

After deployment, diagnose explicitly:

```sh
/Users/joey/.local/bin/clockwork --version
/Users/joey/.local/bin/clockwork doctor
/Users/joey/.local/bin/chancery show clockwork.schedule.operate
/Users/joey/.local/bin/chancery doctor
```

Doctor initializes only an empty unversioned schema-one Clockwork store, refuses
foreign or unsupported schemas, and may mark retained
`running` activations `lost` after proving their recorded processes absent. It
does not execute a product job. Runtime exit evidence does not establish
product-domain success.

## Rollback

A deployment failure before commit restores the exact prior current and
previous selectors plus public command and provider views. If coherent
restoration cannot be completed, it detaches all owned public selectors and
reports that fail-closed state while retaining releases. After a committed
deployment, resolve `install/previous` to its canonical owned release directory.
Use a trusted tested Rust installer to verify and select that retained release:

```sh
<TRUSTED_CLOCKWORK_INSTALL> recover \
  --release <VERIFIED_PREVIOUS_RELEASE_DIRECTORY> \
  --chancery /absolute/path/to/chancery
```

Do not execute an unverified installer from the retained release.
Program rollback changes the stable Clockwork binary and provider selector.
It does not change a binding or its generated plist. Each plist pins an exact
installed Clockwork binary in a content-addressed release. Do not prune a release
while a plist or running activation refers to it.

## Uninstall selector

The Rust installer's `uninstall` operation removes only Clockwork's owned command,
installer, provider, current, and previous selectors after refusing any remaining
`org.clockwork.*` LaunchAgent plist:

```sh
<TRUSTED_CLOCKWORK_INSTALL> uninstall
```

Before running it, disable every binding and verify quiescence. The uninstaller
serializes with deployment through `/usr/bin/shlock` on the private product
installation lock; that primitive performs atomic PID ownership and safe stale
owner recovery rather than path-renaming a previously inspected lock. When the
Clockwork state root is absent, uninstall may create that empty private root so
the same lock path can serialize against a first deployment; it creates no
runtime database. It does not
boot out or delete a product schedule, kill a running activation, delete
content-addressed releases, remove the database, prune history, or delete
product logs. Retained state and releases require a separate, explicit
destructive operation. Removing selectors is not proof that no already-running
child remains.

## launchd limits

Clockwork supports current-user LaunchAgents only. The user must have a GUI
login domain. Timer delivery and catch-up behavior remain subject to launchd,
login/logout, sleep/wake, clock and time-zone changes, filesystem access and
TCC, and operating-system resource pressure. No readiness check proves the
next delivery time.

## Semantics participation

Clockwork carries the exact marker `Semantics-Project: clockwork` in its
product instructions. This change intentionally does not register or seed the
project. Under separate authority, register the canonical folder, atomically
seed [the project-local definition list](semantics-seed.md) at revision zero,
verify repository HEAD, and then remove the seed source if it is no longer
needed. Until registration, Cell remains the maintained shared-terminology
authority.

# Program installation, retained state, and compatibility

Use this feature to understand installed releases, public selectors, state
compatibility, diagnosis effects, and recovery limits. Clockwork packaging owns
its release tree, command and installer selectors, and only its Chancery
provider selector. Each product owns its definitions, work, pauses, and recovery.
Use `clockwork.install.operate` for ordered operating procedures.

## Interfaces and installation inputs

```text
<TESTED_CLOCKWORK_INSTALL> install --binary ABSOLUTE_PATH --bundle ABSOLUTE_BUNDLE_PATH --chancery ABSOLUTE_PATH [--home ABSOLUTE_HOME] [--expected-current absent|releases/HASH]
<TRUSTED_CLOCKWORK_INSTALL> recover --release ABSOLUTE_OWNED_RELEASE_DIRECTORY --chancery ABSOLUTE_PATH
<TRUSTED_CLOCKWORK_INSTALL> uninstall [--home ABSOLUTE_HOME]
clockwork [--json] doctor
clockwork [--json] migrate --backup ABSOLUTE_NEW_DIRECTORY
./deploy.sh clockwork
```

The `clockwork` CLI prints plain text by default. Pass `--json` for its
existing compact machine response. The separate installer protocol is
unchanged.

The Rust installer accepts the binary, installer, and provider files at absolute
paths. It places those files and publishes their selectors without comparing
versions, validating provider contents, or probing runtime readiness.

Direct installation creates no runtime database, registers no product
schedule, changes no binding, writes no `org.clockwork.*` plist, and runs no
product. It can create missing current-user `.local/bin` and Chancery parent
directories. It validates existing shared parents without changing their modes
and changes modes only within Clockwork's owned installation/state tree.
Root or system installation, remote installation, and foreign-path takeover
are unsupported. An intentional alternate-home or isolated boundary uses the
explicit absolute home option; runtime isolation is not a new production owner.

## Release identity and selector consistency

The installer hashes the exact binary, Rust installer, public layout, and
complete provider tree into one immutable release under
`$HOME/Library/Application Support/Clockwork/install/releases`. Its
`cell-install-v2` manifest is `manifest.json`. The retained installer appears
at `bin/clockwork-install` and `package/install`. The installer reads retained release metadata when selecting an existing
installation or recovering a retained release.

Provider files are copied with the release. Installation does not invoke a
Chancery reader or validate discovery. No runtime state belongs in the release.

One atomic current selector connects both public views:

```text
~/.local/bin/clockwork
  -> .../Clockwork/install/current/bin/clockwork
~/.local/bin/clockwork-install
  -> .../Clockwork/install/current/bin/clockwork-install
.../Chancery/providers/clockwork
  -> .../Clockwork/install/current/share/chancery/clockwork
```

An update retains owned selector boundaries and release metadata. Identical
installation is idempotent. A changed candidate preserves a valid prior
selection as `previous` and atomically replaces `current`. Optional
`--expected-current` enforces the caller's captured absent or `releases/HASH`
expectation. Foreign public paths and selectors outside the owned installation remain
unsupported.

A failed publication before commit restores prior current,
previous, command, installer, and provider views. If coherent restoration
cannot be completed, all owned public selectors are detached and the
fail-closed state is reported while releases remain. Diagnostics and retained
selectors supply recovery evidence; replacing a foreign path is unsupported.

After commit, recovery reads metadata and selects an owned retained release.
Select the retained installer explicitly. Program rollback changes program/provider
selection but leaves product bindings and generated plists unchanged. Each
plist pins an exact content-addressed broker; releases cannot be pruned while
any plist or running activation may refer to them.

## Coordinated broker refresh

Coordinated `./deploy.sh clockwork` captures the complete binding inventory
before maintenance. It disables those bindings while retaining selected
definitions and failure halts. Product adapters prepare new definitions under
their own holds. After holds are released, it restores captured enabled intent
through the final selected broker, rewriting those plists with its exact path.
Previously disabled bindings stay disabled. This phase precedes EMT activation.

An interrupted deployment retains its original inventory and re-establishes
suspension before configuration repair. Temporary disablement does not replace
original intent. Unknown binding or projection changes stop recovery. No
installation phase approves incidents or retries failed work.

## Detach and retained-state scope

Uninstall requires every binding disabled and quiescent. It refuses any regular
or symbolic `~/Library/LaunchAgents/org.clockwork.*.plist`. Plist absence alone
cannot prove a manual child absent. It serializes with deployment through
`/usr/bin/shlock` on the private installation lock, using that primitive's
atomic live/stale PID decision.

Uninstall removes only owned `clockwork`, `clockwork-install`, provider,
current, and previous selectors. If the state root is absent, it may create an
empty private root for the shared lock path; it creates no runtime database.
It does not boot out or delete a schedule, kill a child, delete releases,
remove the database, prune history, or delete product artifacts or logs.
Retained-state deletion and release pruning require separate exact destructive
authority and proof that no activation or generated plist refers to the target.

## State and diagnostic meaning

Runtime state is normally
`$HOME/Library/Application Support/Clockwork/clockwork.db`, with private locks
and notification sidecars beside it. Clockwork broker logs belong under
`~/Library/Logs/Clockwork`. Runtime paths, definitions, schedules, hashes,
process times, and identifiers remain private to the current user. Releases
contain no product secret, output body, definition, or activation history.

A fixed `clockwork_meta` marker identifies the store schema. Clockwork
initializes only an empty unversioned file and refuses unknown versions or
missing schema-two objects. It does not relabel or complete an incompatible
store. Schema two retains definitions, bindings, activations, abends, and
incidents. Program installation does not open, initialize, migrate, delete,
or prune that store.

Doctor opens the schema-two store, prepares private owned paths, runs SQLite
`quick_check`, resolves the current executable and `/bin/launchctl`, and
reports pending transition keys. It can mark running activations lost only
after proving their broker and any child absent. It chooses no journal repair,
executes no product, and changes no binding. Its evidence is local to invocation;
it establishes neither product success nor future timer delivery.

## Explicit migration and rollback

Clockwork 0.5 and later require SQLite schema two. Schema-one upgrade is an
explicit `migrate --backup` operation after product schedules and Clockwork
commands are quiescent and old running rows/transitions have been settled.
Migration takes the schema gate, refuses retained running rows, checkpoints
SQLite, writes a private database-plus-sidecar backup to a new absolute
directory, and changes the schema transactionally. It preserves definitions,
selection, activation history, timers, and product pauses. Program deployment
never performs migration.

Schema-one definitions keep their original digest and legacy failure behavior.
They do not acquire policy merely because storage migrated. Product rollout
registers and selects schema-two definitions under maintenance, preserves
inactive intent, and imports failure-owned halts before removing old gates.
`failure_policy_active: false` exposes an unconverted selection. Enabled
plists need the compatible exact broker before maintenance release. Installation
cannot invoke incident resume or clear product user pauses and recovery evidence.

An old binary cannot open the schema-two store. Cross-schema rollback needs
quiescence, a matching schema-one database and sidecars, compatible Clockwork
and product releases, prior definitions, and generated plists. Failed state
and newer incidents remain recovery evidence. A pre-halt backup cannot erase
a later halt or authorize work.

The hidden absolute test-state override uses `STATE_ROOT/email` as the default
Email double; explicit `failure.email_cli` still selects its authorized wrapper.
Ordinary product reports retain the canonical installed state and Email default.

## Notification compatibility and limits

Shared checks require installed `$HOME/.local/bin/iatreion` and a stable Cell
checkout, default `$HOME/rust/cell`. Notification policy selects another
absolute checkout. Active pinned brokers must understand check eligibility
and EMT handoff; stable CLI replacement alone does not refresh them. Additive
incident feed and routing metadata leave SQLite schema two unchanged.

`failure-checks.json`, `notification-checks.json`, `notification-routing.json`,
the incident database, and EMT correspondence must be backed up and restored
together. The schema-one failure-check sidecar retains the immutable abend ledger
cursor and pending per-key episodes; SQLite remains schema two. Refresh every
enabled pinned broker before relying on the new delay. An older broker must not
run while this sidecar exists. Existing incidents remain halted and preserve
their current notification-check progress, delivery attempts and approval rules.

Older brokers ignore check or claim metadata; restoring them with delegated
ownership can duplicate accepted mail. Removing sidecars to bypass a pending
episode or force another alert is unsupported. Notification configuration does
not resume schedules or retry product work.

Current-user LaunchAgents require a GUI login domain. Clockwork provides no
future timer-delivery or product-readiness promise. Program version, provider
release, feature contracts, manifest schema, store schema, and immutable product
definitions are independent compatibility axes. No automatic release pruning,
retention horizon, deprecation period, availability percentage, or cross-version
migration window is promised. Read `clockwork.bindings` and
`clockwork.notifications` for their detailed guarantees.

# Program installation, retained state, and compatibility

Use this feature to understand installed releases, public selectors, state
compatibility, diagnosis effects, and recovery limits. Clockwork packaging owns
its release tree, command and installer selectors, and only its Chancery
provider selector. Each product owns its definitions, work, pauses, and recovery.
Use `clockwork.install.operate` for ordered operating procedures.

## Interfaces and installation inputs

```text
<TESTED_CLOCKWORK_INSTALL> install --binary ABSOLUTE_PATH --bundle ABSOLUTE_BUNDLE_PATH --chancery ABSOLUTE_PATH [--home ABSOLUTE_HOME] [--expected-current absent|releases/ID]
<TRUSTED_CLOCKWORK_INSTALL> recover --release ABSOLUTE_OWNED_RELEASE_DIRECTORY --chancery ABSOLUTE_PATH
<TRUSTED_CLOCKWORK_INSTALL> uninstall [--home ABSOLUTE_HOME]
clockwork [--json] doctor
clockwork [--json] migrate
./deploy.sh clockwork
```

The `clockwork` CLI prints plain text by default. Pass `--json` for its
existing compact machine response. Direct installer commands retain their
interfaces. Manifest deployment invokes `clockwork-install deploy` with JSON
artifact paths and settings on stdin and uses its exit status as completion.

The Rust installer accepts the binary, installer, and provider files at absolute
paths. It places those files and publishes their selectors without comparing
versions, validating provider contents, or probing runtime readiness.

Direct installation and recovery require every existing binding disabled.
They use the ordinary disable boundary to wait for admitted activations and
refuse unknown process state. They preserve selected definitions, disabled
intent, and halts. They create no runtime database or product schedule and run
no product. Do not start manual activations during replacement. It can create missing current-user `.local/bin` and Chancery parent
directories. It validates existing shared parents without changing their modes
and changes modes only within Clockwork's owned installation/state tree.
Root or system installation, remote installation, and foreign-path takeover
are unsupported. An intentional alternate-home or isolated boundary uses the
explicit absolute home option; runtime isolation is not a new production owner.

## Release identity and selector consistency

The installer copies the exact binary, Rust installer, public layout, and
complete provider tree into one immutable UUID release under
`$HOME/Library/Application Support/Clockwork/install/releases`. Its
`cell-install-v4` manifest is `manifest.json`. The retained installer appears
at `bin/clockwork-install` and `package/install`. The installer reads retained release metadata when selecting an existing
installation or recovering a retained release.

Provider files are copied with the release. Installation does not invoke a
Chancery reader or validate discovery. No runtime state belongs in the release.

The installer copies executable payloads to regular files in the fixed
`install/runtime` tree. Each file replacement is atomic. Public executable
selectors use this tree; the provider selector uses the selected retained
release. `current` and `previous` preserve archive selection metadata:

```text
~/.local/bin/clockwork
  -> .../Clockwork/install/runtime/bin/clockwork
~/.local/bin/clockwork-install
  -> .../Clockwork/install/runtime/bin/clockwork-install
.../Chancery/providers/clockwork
  -> .../Clockwork/install/current/share/chancery/clockwork
```

An update retains owned selector boundaries and release metadata. Each new
preparation has a UUID. Publication preserves a valid prior
selection as `previous` and atomically replaces `current`. Optional
`--expected-current` enforces the caller's captured absent or `releases/ID`
expectation. Foreign public paths and selectors outside the owned installation remain
unsupported.

A failed publication retains completed selector changes and release files.
There is no automatic selector restoration or detachment. Inspect current
selectors and use an explicitly selected recovery operation when authorized.
Replacing a foreign path is unsupported.

Recovery reads metadata and republishes an owned retained release into the
fixed runtime. Use a compatible trusted installer. Program rollback preserves
product bindings and plist bytes; plists with the fixed runtime path then use
the restored broker. Legacy plists retain their archive path until an explicit
binding refresh. Do not prune an archive while a definition, legacy plist, or
running activation can refer to it.

Pre-runtime Clockwork releases remain readable as installation and history
metadata, but are not recovery candidates. Their brokers require an archive
execution path and cannot operate from the fixed runtime or admit schema-three
definitions. `clockwork-install recover` rejects a target older than
`cell-install-v4` before publication. Rebuild historical source with fixed-runtime
support when compatible historical behavior is required.

## Coordinated broker refresh

Manifest `./deploy.sh clockwork` first records the complete binding inventory
in the deployment run's private `clockwork-binding-intent.json`. It disables
every retained binding through the installed broker. Disable waits for an
admitted broker and child to finish; unproved child exit stops replacement.
This includes a manual activation of a disabled binding.

After drain, deployment publishes the fixed runtime files and selects the
same definition for each previously enabled binding through the new broker.
Previously disabled bindings remain disabled. The generated plist uses the
fixed physical broker path. Definitions and incidents are preserved. A
run-at-load definition can start work when enabled intent is restored.

Failure retains completed changes and the captured intent. Unrestored bindings
stay disabled; the executor does not infer retry or recovery. Inspect the
private intent file, current selection, and process evidence before an explicit
recovery. Do not start manual activations during replacement.

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

## Explicit schema migration

Clockwork 0.5 and later require SQLite schema two. Schema-one upgrade is an
explicit `migrate` operation after product schedules and Clockwork
commands are quiescent and old running rows/transitions have been settled.
Migration takes the schema gate, refuses retained running rows and pending
binding transitions, and changes the schema transactionally in place. It
preserves definitions, selection, activation history, timers, and product pauses.
Program deployment never performs migration.

Schema-one definitions keep their original digest and legacy failure behavior.
They do not acquire policy merely because storage migrated. Product rollout
registers and selects schema-two definitions under maintenance, preserves
inactive intent, and imports failure-owned halts before removing old gates.
`failure_policy_active: false` exposes an unconverted selection. Enabled
plists need the compatible exact broker before maintenance release. Installation
cannot invoke incident resume or clear product user pauses and recovery evidence.

An old binary cannot open the schema-two store. Recover program selection only
with a release compatible with retained state. Migration has no reverse schema
operation. Preserve failed state and newer incident evidence during recovery.

The hidden absolute test-state override uses `STATE_ROOT/email` as the default
Email double; explicit `failure.email_cli` still selects its authorized wrapper.
Ordinary product reports retain the canonical installed state and Email default.

## Notification compatibility and limits

Shared checks require installed `$HOME/.local/bin/iatreion` and a stable Cell
checkout, default `$HOME/rust/cell`. Notification policy selects another
absolute checkout. Active pinned brokers must understand check eligibility
and EMT handoff; legacy archive-bound plists require a binding refresh; replacing the fixed
runtime updates the broker used by already converted plists. Additive
incident feed and routing metadata leave SQLite schema two unchanged.

`failure-checks.json`, `notification-checks.json`, `notification-routing.json`,
the incident database, and EMT correspondence remain coherent retained state. The schema-one failure-check sidecar retains the immutable abend ledger
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

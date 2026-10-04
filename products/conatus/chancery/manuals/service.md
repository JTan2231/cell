# Installation, maintenance and scheduled activation

Program publication copies the selected release into fixed regular files beneath
~/Library/Application Support/Conatus/install/runtime. Public commands use that runtime
tree; current and previous retain immutable UUID archive selections. Code signing and
runtime path identity are separate from release identity.

New Clockwork definitions use schema 3: they retain the archive release ID, root and
exact hashes, and execute the fixed runtime image. Publication precedes registration.

Before publication, deployment runs the disable transition for owned Clockwork bindings
and waits for their active processes, including an active manual run on a disabled
binding. It restores saved enabled intent after registration; a failed instruction can
leave the owned bindings disabled.

Existing history, delivery records, enabled intent and incident halts retain their
meaning. Retained definitions and wrapper bytes from before this change keep their
legacy execution paths until a new installation or definition selects the runtime image.

Direct install and recovery require both owned Clockwork bindings disabled. They reject
enabled bindings and run the disable transition for each retained disabled binding to
wait for active processes before runtime publication. They do not select new
definitions; select the matching definition before enabling a recovered or updated
program.

Use this feature to understand Conatus release selection, maintenance admission
and its two independent Clockwork schedules. Use `conatus.update.operate` for
installation, activation and recovery steps. Installation, initialization,
schedule registration and activation are separate effects.

## Installed releases

```sh
conatus-install install --binary ABS_BINARY --bundle ABS_BUNDLE --expected-current absent
conatus-install inspect
conatus-install recover --release ABS_RELEASE --expected-current releases/ID
```

Packaging commands accept `--home ABS_HOME`. The default installation root is
`~/Library/Application Support/Conatus/install`. Install stages a
immutable executable release and its matching Chancery bundle, then
selects that candidate. Upgrade uses the observed `releases/ID` in place of
`absent`. The complete provider bundle includes the overview, indexed entries
and complete manuals; its bytes participate in release identity.
The provider selector follows the product's selected release.

Direct installation starts no model, initializes no runtime state and changes
no active schedule. Recovery selects an exact retained release. It does not
revert Conatus state, Annals libraries, frozen email occurrences or Clockwork
bindings. Installation does not check artifact integrity, persistent-state
integrity, or operational readiness. Ordinary product checks remain separate.

Want lifecycle remains in schema-one current state without a migration. Older
releases ignore archive markers and can include archived wants in default lists
and new email. Use a lifecycle-aware release when filtering is required. Retained
email occurrences keep their original bytes across format changes.

## Exact schedule definitions

```sh
conatus-install schedule-definition --state-dir ABS_STATE --output ABS_NEW_FILE
conatus-install schedule-definition --daily-email --state-dir ABS_STATE --output ABS_NEW_FILE
```

Definition generation uses the selected installed release. The state directory
and output parent must exist, and the output file must be new. It writes the
definition and creates the private state log directory. Its receipt separates
the key, release identity and output path from `registered:false` and
`activated:false`. Definitions contain selected paths and no credential.

`conatus/update` uses definition schema 3, every 300 seconds, run-at-load enabled
and overlap skip. It retains the immutable archive identity, pins the runtime executable, and invokes
`conatus --state-dir ABS_STATE update`. Clockwork opens product-owned
`logs/update.out.log` and `logs/update.err.log`; Conatus owns their contents.
The definition uses an explicit home and minimal system search path.

`conatus/daily-email` is separate. It uses local 09:00, no run-at-load, overlap
skip and a 180-second activation limit. Enabling it grants standing authority for
the exact daily content described by `conatus.digest.email` to Email's fixed
personal recipient. It starts no model and does not invoke update.

Clockwork owns immutable registration, selection, launchd activation, process
history and scheduling failure incidents. Login, sleep and launchd affect actual
activation. Direct program installation does not retarget an existing immutable
definition. Do not keep old and new inbox schedules active together. No maximum
start delay, catch-up guarantee or domain completion time is promised.

## Failure halts

Both definitions select `[failure] on_abend = "halt-until-approved"`.
On the first feed, handoff or inbox error, update preserves completed results,
retains its report and returns nonzero. Annals dispatch stops at its first failed
source. Clockwork retains a pending failure episode and permits later scheduled
activations before the shared service-health threshold. By default, five
consecutive failed read-only checks, at least 60 seconds apart, halt the affected
binding and make its alert eligible together. Healthy or inactive checks clear
a pending episode. Checks do not retry failed sources or uncertain email.

Only explicit `clockwork binding resume KEY INCIDENT_ID` releases the incident.
A definition switch, deployment, Conatus resume or Annals recovery cannot clear
it. Schema-one bindings acquire this policy only after an explicit schema-two
definition switch. Generating a definition does not activate the policy.

A Clockwork continuation permits future scheduling. It does not retry a failed
Annals attempt or an uncertain email, clear another binding's halt, or establish
domain success. Product operator pause, Annals pause, bounded retry-event halts,
source identities and receipts remain product-owned. Resolve uncertainty and
the failure cause before requesting continuation.

## Maintenance and installation instructions

```sh
conatus --json config
conatus maintenance status
conatus maintenance hold OWNER
conatus maintenance drain
conatus maintenance release OWNER
```

Config and maintenance commands print readable labeled results by default.
Use `--json` for their existing machine responses. Human and JSON errors go to
stderr with a nonzero exit; JSON errors retain `{ "ok": false, "error": "..." }`.

Config reads persistent dependency and library selections only. Maintenance
exposes owner-scoped durable admission holds and drain. Holds survive interruption.
Send holds the product admission guard. Deliberate scheduled admission during
maintenance returns a successful maintenance skip. A hold does not authorize
release of another owner's hold or approval of a Clockwork incident.

`./deploy.sh conatus` executes the declared `conatus-install deploy` instruction.
The installer publishes programs, initializes absent state or updates the final
Annals executable path. Existing library identities, cursor, records,
instructions and product pause remain unchanged. It uses ordinary product
admission and runner locks. It creates no maintenance hold. It suspends owned
schedules and waits for active processes before runtime publication. It does not
drain durable work or perform a health or signature audit.

Settings accept absolute `state_dir`, `decisions_config` and `annals_state_dir`,
plus `library`, update `enabled` and independent `daily_email_enabled`.
Existing library selections cannot change during deployment. Fresh defaults use
the Conatus state root, installed Annals state root and its `decisions/config.toml`,
and library `conatus`. Other selections must be supplied explicitly.

An absent binding remains absent unless its enabled setting is supplied.
Omitted email intent preserves the prior daily-email selection and intent.
Existing definitions retain timer, arguments, environment and output paths.
The installer updates the selected executable and Clockwork execution pin,
then publishes each definition with its intended enabled state. Existing failure
halts remain in force. Setup starts no model processing or email submission.

Failure leaves completed installation effects in place. Interruption requires
inspection before an explicit next operation; the executor does not repeat,
roll back or recover instructions. Explicit retained-release selection changes
programs only and does not revert domain state or authentication. Read the
shared operator manual for executor failure and interruption handling.

## Authority, privacy and compatibility

Conatus owns packaged release selection, product maintenance, generated runner
definitions and product output. Clockwork owns activation and incidents. Annals
owns dependency and domain readiness; Email owns submission transport.
A selected release, catalog entry or exit-zero process does not establish
retention, interpretation, email delivery or current readiness.

Installation and definitions use current-user local paths. Private sources,
frozen messages, logs and dependency context remain under their product owners.
Provider documentation contains no credentials or private runtime records.
The supported feature supplies no cleanup or pruning policy for those records.

Provider schema 4 publishes the overview; the release, entry contract, Conatus
state schema and Clockwork definition schema are separate versions. No general
cross-release support interval, installation latency, history retention horizon,
throughput or storage-capacity guarantee is promised.

## Command usage

Run `conatus --register-usage` after installation or update. It registers command
inventory without product work. CLI usage recording requires a nonempty
`CODEX_THREAD_ID`. Chancery records command identity, time and thread ID, not
arguments, output or outcomes. Internal calls are excluded. Recording errors
preserve product results.

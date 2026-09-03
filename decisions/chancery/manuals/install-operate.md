# Install and operate Decisions

Read `docs/system-installation.md` in the matching release. Deploy only a green
candidate and a compatible installed Clockwork frontend. Decisions uses two
stable Clockwork binding keys and owns one exact user hook:

An explicit home override must be an absolute, nonsymbolic directory owned by
the current operator. Values containing `&`, `<`, `>`, `|`, double quote,
backslash, or newline are refused because that path is rendered into exact XML
and TOML schedule inputs.

Deploy and uninstall serialize through the same private
`install/.update-lock` directory. Uninstall may create otherwise absent private
state/install directories solely to contend for that lock and removes them
again when they remain empty.

- `decisions/observer` runs one serial observation every 60 seconds;
- `decisions/daily-email` projects and sends at local 09:00; and
- `~/.codex/hooks.json` synchronously invokes `decisions observe ingest` for
  `Stop` with a three-second timeout.

Neither binding uses `RunAtLoad`. Each immutable definition names an exact
release-owned runner plus explicit interpreter and digests; Clockwork receives
only literal non-secret environment, retains no output body, and generates a
plist whose program arguments its own contract limits to its absolute frontend
path, private entry, and stable key.
Decisions establishes binding ownership from the selected immutable definition,
not from generated plist shape alone. Both runners
then create scrubbed, key-free environments and write separate body-free
Decisions logs. Before doing so, each checks the private, release-independent
`~/Library/Application Support/Decisions/.clockwork-maintenance` gate. The hook
persists only session/turn correlation and performs no model work. A runner
invocation that observes a valid current-user-owned mode-`0600` gate exits
successfully without dependency resolution or domain work; invalid marker
state fails closed. The marker does not terminate work already past that check,
so deployment separately proves database quiescence before migration.

The deployer proves selector, legacy-plist, loaded-label, and hook ownership
before mutation. A legacy plist must byte-match the current format-2 release's
complete rendered template and be current-user-owned mode `0644`; its label and
runner alone do not establish ownership. The deployer treats a selected
Clockwork binding as owned only after
`definition show` matches the complete exact current-release manifest; foreign
or ambiguous selected definitions are refused. That point-in-time proof is not
a Clockwork compare-and-swap; Decisions serializes its own lifecycle tools, and
concurrent direct same-user mutation of either key is unsupported and may
force maintenance-gated recovery. A pre-existing user `hooks.json` is foreign and is never merged or
overwritten. An installed hook must remain byte-identical to its owning release
for update or uninstall. Other hook layers can coexist because Codex loads them
separately.

For update, a private current-user-owned mode-`0600`, non-hard-linked
maintenance gate is engaged
before an immutable definition is registered, either Clockwork binding is
changed, or a legacy scheduler is mutated; an existing valid gate is never
truncated. Existing product logs must be current-user-owned regular
non-hard-linked files, and deployment restricts their mode to `0600` without
truncating content before registration. Both bindings are then disabled, any
legacy services are quiesced, the public Decisions
command is suspended, and the three-second hook
timeout is drained before the database plus SQLite
sidecars are backed up. Candidate doctor performs the explicit sequential
version-one or version-two to version-three migration. That transaction
preserves old domain rows and backfills the retained candidate/review lifecycle
stream before changing the user version. After the release and selectors are staged, `observe
activate` records the baseline exactly once. Its default is the next whole Unix
second, conservatively excluding authority items timestamped in the cutover
second. Only then are the hook and public command published and the daily and
observer Clockwork bindings switched, observer last. Rollback authority ends
only after all durable state and both switches have committed; the maintenance
gate is removed after that boundary. A legacy direct plist and its Clockwork
schedule never coexist after handoff. A missed Stop
during this short suspension is recovered by reconciliation.
When restoration is provable, failure restores hook bytes, any prior selected
Clockwork definition digest and enabled state, exact legacy service bytes and
loaded states, selectors, database, and sidecars. A formerly absent binding may
become an inert disabled tombstone; Clockwork's disabled-selection recovery form
restores a prior disabled digest without briefly enabling its schedule. If
rollback cannot prove database quiescence or restore every captured artifact,
including after switching a formerly unselected binding, it retains the
maintenance gate and private transaction backup, attempts scheduler cleanup,
and removes the public command when removal can be proven. It does not claim
that either external scheduler was disabled. Redeploy, uninstall, and
reinstall never advance the baseline.

After deployment:

```sh
decisions doctor
decisions observe status
clockwork binding show decisions/observer
clockwork binding show decisions/daily-email
clockwork history decisions/observer --limit 20
clockwork history decisions/daily-email --limit 20
```

Source deferral does not monopolize the worker. Observation processing resumes
an existing processing row first, skips queued rows before their retry time, and
orders ready queued work by retry-ready time. A nonzero queued count can
therefore coexist with no currently ready observation.

If diagnosis proves that one observer-deferred `TurnNotFound`-shape Stop-hook
source is permanently unavailable, obtain explicit recovery authorization and
run:

```sh
decisions observe abandon OBSERVATION_ID --source-unavailable
```

This waits for the serial observation-processing lock and accepts only a
previously deferred pending level-0 row with a retry time, no not-completed
marker, and no bound source, job, authority, verdict, or candidate. It records
audited `complete` / `not_eligible` state with the fixed
`conversation_source_abandoned` marker, stores no caller-provided reason,
creates no lifecycle event, and leaves the observer baseline unchanged. The
exact repeat is idempotent recovery from uncertain command completion. Never
use it for a merely unfinished turn. A changed row is refused, and
completed-root reconciliation fails closed if the abandoned source later
appears. This is a supported serialized state repair, not a reason to edit
SQLite directly or redeploy the services.

Open `/hooks`, review and trust the exact non-managed user `Stop` hook, then
create one deliberate post-baseline effectful turn on the actual Codex surface
in use and verify its observation. An installed file or CLI canary does not by
itself prove Desktop receipt. Never bypass hook trust.

Uninstall of a current release first takes that shared update lock, then
engages and retains the maintenance gate and disables only
definition-proven Decisions bindings, refuses foreign
or ambiguous selected definitions, and leaves absent or unselected disabled
tombstones alone. It removes exact legacy services, the exact owned hook, and public selectors. It
retains the gate, database, activation baseline, releases, and logs. Delete those only
after a separate explicit retention and recovery decision.

# Install and operate Decisions

Read `docs/system-installation.md` in the matching release. Deploy only a green
candidate. The installer owns two LaunchAgents and one exact user hook:

- `org.decisions.observer` runs one serial observation every 60 seconds;
- `org.decisions.daily-email` projects and sends at local 09:00; and
- `~/.codex/hooks.json` synchronously invokes `decisions observe ingest` for
  `Stop` with a three-second timeout.

Neither service uses `RunAtLoad`; both runners have scrubbed, key-free
environments and separate body-free logs. The hook persists only session/turn
correlation and performs no model work.

The deployer proves selector, plist, loaded-label, and hook ownership before
mutation. A pre-existing user `hooks.json` is foreign and is never merged or
overwritten. An installed hook must remain byte-identical to its owning release
for update or uninstall. Other hook layers can coexist because Codex loads them
separately.

For update, both services are stopped, the public Decisions command is suspended,
and the three-second hook timeout is drained before the database plus SQLite
sidecars are backed up. Candidate doctor performs the explicit sequential
version-one or version-two to version-three migration. That transaction
preserves old domain rows and backfills the retained candidate/review lifecycle
stream before changing the user version. After the release and selectors are staged, `observe
activate` records the baseline exactly once. Its default is the next whole Unix
second, conservatively excluding authority items timestamped in the cutover
second. Only then are both plists, the hook, and the public command published
and the daily and observer services bootstrapped, observer last. A missed Stop
during this short suspension is recovered by reconciliation.
Failure restores hook bytes, both service files and loaded states, selectors,
database, and sidecars. If rollback cannot prove database quiescence or restore
every captured artifact, it leaves both services stopped, disables the public
command, and retains the private transaction backup. Redeploy, uninstall, and
reinstall never advance the baseline.

After deployment:

```sh
decisions doctor
decisions observe status
launchctl print "gui/$(id -u)/org.decisions.observer"
launchctl print "gui/$(id -u)/org.decisions.daily-email"
```

Open `/hooks`, review and trust the exact non-managed user `Stop` hook, then
create one deliberate post-baseline effectful turn on the actual Codex surface
in use and verify its observation. An installed file or CLI canary does not by
itself prove Desktop receipt. Never bypass hook trust.

Uninstall removes both services, the exact owned hook, and public selectors. It
retains database, activation baseline, releases, and logs. Delete those only
after a separate explicit retention and recovery decision.

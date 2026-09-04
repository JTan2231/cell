# Krisis system installation

This document describes the packaged current-user macOS layout. Building a
candidate does not authorize release, deployment, hook trust, live migration,
or a canary.

## Installed identities

- executable and public command: `krisis`
- Chancery providers: `krisis` 0.4.0 and read-only compatibility provider
  `decisions` 0.4.0
- active Clockwork key: `krisis/observer`
- SQLite schema: 4
- Nucleus provider/requester: `krisis`
- account capture rule: `krisis/decision-account-classification/1`

For migration compatibility, persistent paths do not move:

- database and releases: `~/Library/Application Support/Decisions/`
- installed observer ownership receipt:
  `~/Library/Application Support/Decisions/install/krisis-observer-binding.txt`
- logs: `~/Library/Logs/Decisions/observer.stdout.log` and
  `observer.stderr.log`
- maintenance marker:
  `~/Library/Application Support/Decisions/.clockwork-maintenance`

The old public `decisions` command, `decisions/observer`, and
`decisions/daily-email` bindings are retired. Final cutover disables only an
enabled legacy binding whose selected definition is proven to belong to the
current Decisions release. Disabled or foreign legacy definitions are left
untouched. Retained legacy database rows and Clockwork history are not deleted.

## Dependency configuration

The observer needs exact absolute paths to Codex, Annals, and the dedicated
Annals decisions config, plus that library's persistent ID. A packaged prepare
accepts:

```text
deploy-user.sh \
  --binary /absolute/path/to/krisis \
  --clockwork /absolute/path/to/clockwork \
  --annals /absolute/path/to/annals \
  --annals-config "/absolute/path/to/Annals/decisions/config.toml" \
  --annals-library-id 0123456789abcdef0123456789abcdef
```

The library ID must be exactly lowercase 32-hex. Krisis passes the explicit
config to Annals; it never chooses a library by fallback or `--library`.

## Prepare and final cutover

Preparation is the default. It installs the content-addressed release,
registers and fully verifies its Clockwork definition, prepares private logs,
and deliberately leaves the maintenance marker in place. It does not change
the current release, command, provider, hook, database, baseline, or any
Clockwork binding.

The outer cutover operator must then prove the separately managed Annals and
semantic activation prerequisites. Krisis does not infer that proof from a
running Clockwork process. Only then repeat the exact command with
`--final-cutover`.

Final cutover validates every current selector, selected Clockwork definition,
target-bound observer ownership receipt, and legacy plist before mutation. It
disables only proven-owned enabled
schedules, suspends the old hook command for its timeout, proves SQLite
quiescence, and saves the database and sidecars. It then selects the prepared
release, migrates through schema 4, and runs doctor in a scrubbed environment.

Doctor checks Conversations, exact Nucleus capabilities and requester contract,
and:

```text
annals --config CONFIG --json decision-feed watermark
```

It accepts only the standard success envelope with contract version 1 and the
configured library ID. The baseline is created once during explicit final
cutover, before the exact Krisis hook and `krisis/observer` binding become
executable. The deployer rereads the active and legacy bindings after the
switch, proves the exact candidate is enabled and the legacy schedules are not,
then removes the maintenance marker.

The observer definition runs every 60 seconds with `run_at_load = false`, pins
the exact release-local runner and interpreter digest, and uses a scrubbed
environment. The scheduled wrapper suppresses detailed child errors and emits
only a fixed failure message, so it writes body-free output to the existing
Decisions log path. Interactive `krisis` diagnostics remain detailed.

## Verification

After a separately authorized final cutover:

1. Run `krisis doctor` with the installed Annals configuration.
2. Run `krisis observe status` and confirm schema 4 and the write-once baseline.
3. Inspect `krisis/observer` definition, binding, runtime history, and body-free
   logs; confirm both retired Decisions keys are absent or disabled.
4. Inspect and explicitly trust the exact `~/.codex/hooks.json` definition.
5. Only with separate canary authority, complete one synthetic post-baseline
   root user turn and verify binary coverage, then verify account acceptance in
   the configured Annals decisions library.

An installed file or binding does not prove Codex hook delivery or Annals domain
success on its own.

## Recovery and uninstall

Pending Annals delivery is normal recoverable state. Repeated observer runs
submit the exact same producer key, bytes, config path, and library identity
until Annals returns `created` or `replayed`. Do not retry classification for
delivery failure.

Use `observe reconcile` for a missed hook, `observe retry` only after diagnosing
a terminal classification failure, and guarded `observe abandon` only after
proving one still-unbound source permanently unavailable. Never edit SQLite.

If deployment cannot prove exact selector restoration (including Clockwork's
inability to restore a selected definition to null), it disables the owned
candidate and retains the maintenance marker and private transaction backup for
explicit recovery. Uninstall matches the selected definition to the installed
ownership receipt and disables only that exact owned active binding; enabled
foreign or legacy bindings are left
untouched and stop the operation. It retains the database, baseline, receipt
ledger, legacy Decisions history, releases, logs, and Clockwork history.
Deleting those requires a separate destructive decision.

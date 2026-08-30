# User-owned macOS installation

Weaver installs as a user-owned CLI. Each `submit` starts a detached one-shot
worker from the interactive caller; Weaver owns no LaunchAgent, resident daemon,
root-owned file, administrator credential, log service, or Codex credential.
The separately installed Nucleus service owns Codex execution and
authentication.

The direct child boundary is required for local repository access. A live
prototype demonstrated that `org.weaver.worker` under launchd received
`EPERM` when unlinking an output under `~/Documents`, while the interactive
process had the required repository file-access authorization. Installing
another scheduler would reintroduce that privacy-authority mismatch.

The Nucleus service is deliberately outside that repository boundary too. In a
second live canary, a Nucleus-launched Codex app-server whose invocation cwd was
the repository under `~/Documents` remained stuck in `getcwd` before completing
its app-server handshake. Weaver now snapshots stage inputs in the detached
interactive-lineage process and sends their contents in the request. The
Nucleus invocation uses Weaver's private state root as a read-only cwd, with
local execution, web, launch context, and dynamic tools all disabled.

## Layout

```text
~/.local/bin/weaver -> current Weaver release
~/Library/Application Support/Weaver/
  current.json
  .control.lock
  .run.lock
  .maintenance
  install/
    releases/RELEASE_ID/
      bin/weaver
      package/deploy-user.sh
      share/chancery/weaver/
        provider.json
        entries/
        manuals/
      manifest.txt
    current -> releases/RELEASE_ID
    previous -> releases/RELEASE_ID
~/Library/Application Support/Chancery/providers/
  weaver -> ~/Library/Application Support/Weaver/install/current/share/chancery/weaver
```

The state and releases belong to the logged-in user. Weaver does not set or own
`CODEX_HOME`. During an active stage, `current.json` contains the exact request,
including every private stage-input byte embedded in its prompt. Keep the whole
state root private; content-addressed releases contain only Weaver's program,
deployer, and public capability documentation, never repository inputs. The
Chancery bundle's content participates in the release identity, and its
`provider.release` must match the Weaver binary version. Weaver does not invoke
Chancery at runtime. The exact prototype paths below are migration inputs, not
part of the resulting installation:

```text
~/Library/LaunchAgents/org.weaver.worker.plist
~/Library/Logs/Weaver/
```

Deployment removes the exact prototype plist and bootstraps no replacement. It
retains existing prototype logs as inert diagnostics rather than deleting user
data.

## Deploy or update

Install and authenticate Nucleus first. Then build, test, and pass the absolute
candidate path to the deployer:

```sh
nucleus health
cd /Users/joey/rust/cell/weaver
./ci.sh
./packaging/macos/deploy-user.sh \
  --binary "/Users/joey/rust/cell/target/release/weaver"
```

The deployer verifies the candidate version and help, runs `weaver doctor`, and
stages a complete content-addressed release. It then acquires the installation
update lock and asks the active Weaver release to begin maintenance.
Maintenance prevents a racing submission and lets an active complete workflow
finish. The default update wait is 21,600 seconds; set
`WEAVER_UPDATE_WAIT_SECONDS` or pass `--wait-seconds` to choose another
nonnegative bound.

After quiescence, the deployer disables and boots out exactly
`gui/UID/org.weaver.worker` if the prototype service is loaded, captures and
removes exactly `~/Library/LaunchAgents/org.weaver.worker.plist` if present,
and clears the label's disabled override. It atomically switches `current`,
`previous`, `~/.local/bin/weaver`, and Weaver's Chancery provider selector,
validates the installed binary and Nucleus readiness, and proves a worker exits
cleanly under maintenance. It then commits the release and ends maintenance.
Nothing is registered with launchd.

The deployer owns only
`~/Library/Application Support/Chancery/providers/weaver`; it preserves every
other provider entry. Its selector always has the exact stable target shown in
the layout, so switching `current` also switches the resolved immutable bundle.
If that selector already exists as a regular file or points anywhere else, the
deployer fails instead of taking it over.

A failed pre-commit cutover restores the prior release, command, and provider
resolution. If the prototype was present, it also restores the exact plist and
loaded-service state. The transaction retains `current.json`, generated
narrative outputs, Nucleus state, prototype logs, unrelated Chancery providers,
and every staged content-addressed release. An identical package reuses its
existing release directory after validating every recorded hash, including the
complete Chancery bundle.

## Activation and operation

`submit` persists the current workflow before spawning a detached copy of the
same executable:

```sh
weaver --repo '/absolute/path/to/repository' submit NARRATIVE
weaver status RUN_ID
weaver wait RUN_ID
```

The child runs `--state-dir ABSOLUTE_PATH worker run` from the private state
root with null standard streams and a new process group. It retains the
interactive caller's macOS file-access attribution, performs all repository I/O,
and exits after the current workflow becomes terminal or encounters a
recoverable boundary. For each stage it sends Nucleus an immutable request that
contains the selected inputs rather than asking the service-launched Codex
process to enter the repository. No scheduler restarts it at login.

A nonterminal `wait` activates a worker immediately and every 30 seconds, so it
is the explicit recovery command after a crash, logout, or restart. `cancel`
also activates after durably recording cancellation. `status` is read-only and
does not start a process.

Inspect application and runtime health with:

```sh
weaver doctor
weaver status
nucleus jobs list --requester weaver --requester-id RUN_ID
```

Detached worker diagnostics are written into the current workflow detail rather
than a separate Weaver log. Nucleus retains its own job and raw protocol logs;
its database contains complete stage prompts and may therefore retain the
private basis, brief, original sources, and generated inputs embedded by
Weaver. Keep it inside Nucleus's documented private boundary.

## Maintenance recovery

An ordinary failed deployment before commit removes maintenance and restores
the old installation. If interruption occurs after the new release commits but
before maintenance can end, the new release remains authoritative and work
stays safely gated. End that exact gate with:

```sh
WEAVER_STATE_DIR="$HOME/Library/Application Support/Weaver" \
  "$HOME/.local/bin/weaver" maintenance end
weaver wait RUN_ID
```

Do not remove `.maintenance` or edit `current.json` by hand. Do not restart or
replace Nucleus to recover a Weaver worker; `wait` can start a new requester
process and rediscover its durable Nucleus job, while a Nucleus restart makes an
active attempt lost.

If deployment times out waiting for an active run, inspect `weaver status` and
the correlated Nucleus job. Either let that workflow finish or explicitly
cancel its run, understand the terminal result, and repeat deployment. Never
cancel a Nucleus job merely to shorten routine maintenance.

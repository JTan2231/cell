# Install and operate Weaver

Use the Cell deployment coordinator to install or update Weaver. The command,
installer, provider, and private application directory are named Weaver. Its
Cell source is `weaver-narrative`; Semantics project `weaver-narrative` is
separate from the permanently retired predecessor `weaver` registration.

## Install

Run the default `./ci.sh` gate before deployment. Commit the tested candidate
to local `main`. The coordinator selects that commit; it does not publish Git
changes or author a document.

```sh
./deploy.sh plan weaver
./deploy.sh weaver --settings /absolute/weaver-settings.json
```

The first installation requires a private settings file with this shape:

```json
{"weaver":{"annals_config":"/absolute/Annals/decisions/config.toml"}}
```

`annals_config` is the only setting. It must select an existing identity-bound
Annals decisions library. Weaver does not provision that library. Later
deployments reuse the stored path unless settings select another path.
The installed reader is the current user's `~/.local/bin/annals`.
Compatible Annals and Nucleus releases are installation dependencies.

Inspection reads the source configuration and Annals start cursor without
changing either library. The coordinator holds and drains Weaver, selects the
immutable program/provider release, initializes absent Weaver state or checks
schema 1, and verifies the database, Annals reads, and Nucleus readiness.
It releases only its own hold. No model job or document is created by readiness
verification. Weaver defines no Clockwork binding or service.

## Inspect and configure

```sh
weaver doctor
weaver config
weaver init --annals-config /absolute/decisions/config.toml
weaver init --annals-config /absolute/decisions/config.toml --annals-binary /absolute/annals
weaver maintenance status
weaver maintenance hold RUN_ID
weaver maintenance drain
weaver maintenance release RUN_ID
weaver-install inspect
weaver-install verify-release /absolute/Weaver/install/releases/RELEASE_ID
```

Init creates schema 1 only in an absent or empty database and atomically saves
reading configuration. Existing documents remain unchanged. Unsupported state
is refused. Reconfiguration takes the same runner lock as writing.
Doctor reads database integrity and source readiness and checks the required
Nucleus capabilities. It creates no domain records or model jobs.

Doctor uses ordinary admission when Weaver has no maintenance hold, even if a
caller supplies `CELL_DEPLOYMENT_RUN_ID`. When Weaver is held, doctor requires
that ID to match its sole hold. It keeps the ID for Nucleus deployment readiness.

Maintenance returns `maintenance.protocol_version=1`, `holds`, `drained`, and
`nonterminal_jobs`. Drain requires no admitted Weaver process and no nonterminal
Weaver Nucleus job. An unavailable job inventory remains unknown and cannot
prove drain. A hold blocks new writes and revisions; resume can settle an
existing exact assignment. Hold and release preserve other owners' holds.

The installer uses the shared Cell content-addressed file transaction and
maintained adapter. Public binary and Chancery selectors follow the selected
release. It refuses foreign selectors, altered candidates, and unsupported
legacy installation formats. Direct installer `install` and `recover` are
refused; use the coordinator. Read-only inspection and verification remain
available. Uninstall detaches owned selectors and retains private state.

## Recovery and limits

An interrupted deployment uses the coordinator's retained transaction recovery.
After candidate publication, Weaver can finish forward with the recorded
reading configuration and supported database. Before publication, the prior
installation remains selected. Unproved recovery retains the named hold.
Do not delete a hold or edit SQLite to make deployment proceed.

Schema 1 has no predecessor migration. The retired Weaver's workflow records
are not imported or replaced. Keep `weaver.sqlite` and `config.json` private;
use a consistent SQLite backup or copy the database while Weaver is drained.
Keep Nucleus records and credentials under Nucleus's separate backup rules.
Installation publishes program bytes locally. It does not publish narratives,
send email, retry failed jobs, or guarantee future model availability.

`status-snapshot --json` declares `weaver/author` as on demand. Its incomplete
snapshot does not claim live readiness. Use doctor and maintenance for that
evidence. No latency, retention horizon, or release cadence is promised.

After deployment, verify `weaver --version`, `weaver doctor`, the exact installed
release, and `chancery show weaver.narrative.write`. Run
`weaver --register-usage` to register its command inventory. These checks do not
create a narrative.

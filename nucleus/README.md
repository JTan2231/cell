# Nucleus

Nucleus is a per-user, local job coordinator for agent harnesses. A requester
submits a small, versioned invocation contract; Nucleus validates it against the
installed harness, supervises the process, owns its authentication, and retains
one exact observation for every non-sensitive harness stdout JSONL record.
Host-managed authentication responses and managed-worker stderr are excluded
so bearer credentials cannot enter durable job output. Requesters keep
ownership of their domain work and execute any domain tools through Nucleus's
durable mailbox.

The daemon admits work durably and runs up to eight Codex jobs concurrently.
Additional jobs remain accepted until a slot opens. Nucleus does not interpret
workflow dependencies or detect shared mutation targets; requesters must
isolate or serialize conflicting work.

The first adapter is Codex app-server on macOS. Nucleus does not accept shell
commands, arbitrary argv, retries, or workflow graphs. A requester that needs
caller-process environment parity can register a short-lived, memory-only
launch context; those values never enter the job or log database.

For operating the installed system or changing a boundary shared with Annals,
Todo, or Codex, start with [the operator manual](docs/operator-manual.md). An
installed release also makes its version-matched manual available with
`nucleus manual`.

The [release-matched Chancery provider](chancery/provider.json) publishes
Nucleus's complete outward inventory for execution and service operation,
requester integration, and product development. After Chancery discovery
selects an exact entry, use `chancery resolve ENTRY_ID` to assemble its scoped
promise, substantive reliances, dependency contracts, exact basis, and
explicit gaps. Resolution does not inspect live Nucleus readiness or execute a
job or service action.

## Build and run

```sh
cd /Users/joey/rust/cell
/Users/joey/.cargo/bin/cargo build --release \
  --package nucleus-cli \
  --package nucleus-daemon
/Users/joey/rust/cell/target/release/nucleusd serve
```

By default the daemon listens on the Unix socket at
`~/Library/Application Support/Nucleus/nucleus.sock` and stores SQLite state in
the same directory. For an isolated foreground instance:

```sh
/Users/joey/rust/cell/target/release/nucleusd serve \
  --socket /tmp/nucleus.sock \
  --database /tmp/nucleus.db \
  --codex /opt/homebrew/bin/codex \
  --codex-home /tmp/nucleus-codex-home
```

Install it as the current user's always-on LaunchAgent through the packaged
deployer so the executable release and its Chancery provider bundle are staged
together:

```sh
/Users/joey/rust/cell/nucleus/packaging/macos/deploy-user.sh \
  --binary /Users/joey/rust/cell/target/release/nucleus \
  --daemon /Users/joey/rust/cell/target/release/nucleusd \
  --codex /opt/homebrew/bin/codex \
  --codex-home "$HOME/path/to/current-signed-in-codex-home"
/Users/joey/rust/cell/target/release/nucleus service status
/Users/joey/rust/cell/target/release/nucleus health
```

Installation copies the binaries to `~/.local`, writes
`~/Library/LaunchAgents/org.nucleus.daemon.plist`, and loads it with
`launchctl bootstrap`. The packaging layer first stages an immutable release
containing the exact CLI, daemon, deployer, and Nucleus-owned Chancery bundle
under `~/Library/Application Support/Nucleus/install/releases/`. Its `current`
selector and `~/Library/Application Support/Chancery/providers/nucleus` are
rollback-protected around the existing service transaction. Chancery is not a
Nucleus runtime dependency, and Chancery upgrades do not change that
product-owned provider selector. The daemon remains in the foreground under
launchd. Nucleus CI validates the bundle and requires its provider release to
equal the workspace Nucleus version; `release.sh` bumps both together.
The installer copies `auth.json` from `--codex-home` into
`~/Library/Application Support/Nucleus/codex-home`, writes a minimal private
`config.toml` that selects the file credential store, and configures the
LaunchAgent to use only that Nucleus-owned home. The source home is an import,
not a shared runtime path. Static API-key jobs receive isolated snapshots.
Managed ChatGPT jobs receive only in-memory access tokens from Nucleus; their
401 callbacks are coalesced into one serialized refresh of the authoritative
home, and no worker receives a refresh token or copies authentication back.
Account reads and attended login also run against private staging homes; only a
validated credential generation is atomically promoted into the authoritative
home. Attended login waits for all active authentication sessions before
changing the account, while ordinary account reads and refreshes can coexist
with running jobs.

If Nucleus has no signed-in credential yet, authenticate the owned home with:

```sh
/Users/joey/rust/cell/target/release/nucleus auth login --device-auth
/Users/joey/rust/cell/target/release/nucleus account --wait 0
```

`nucleus health` is strict: it prints the readiness document but exits nonzero
unless the daemon is compatible, authenticated, and accepting jobs.

## Operational storage

Nucleus retains job and attempt authority, immutable registrations, the durable
tool mailbox, and an atomic harness-output ledger in
`~/Library/Application Support/Nucleus/nucleus.db`. Each ledger row contains
only attempt attribution, arrival sequence, observation time, and exact
non-authentication stdout payload bytes. Inputs, host-managed authentication
responses, lifecycle events, stderr chunks, requester-result log rows, and
reporting aggregates are not stored there. Requests, mailbox values, and model
output can still contain sensitive prompts, tool arguments/results, and source
content. There is no automatic output-retention or pruning policy, so operators
should monitor free space and treat the database as sensitive
local state. Back it up with a SQLite-aware backup while the service is stopped
(or include its WAL consistently); copying only the main database file while
the daemon is running is not a complete backup. The LaunchAgent's stdout and
stderr files under `~/Library/Logs/Nucleus` likewise need the host's normal log
rotation policy.

Opening a version-one database performs a coordinated cutover to store schema
version 2. It preserves operational jobs, attempts, registrations, cancellation,
and terminal state, but discards old mixed logs and historical answered mailbox
rows. The cutover refuses to run with a pending requester call whose job and
attempt are still nonterminal; stale terminal-owner calls are
discarded. It then compacts the database before reporting healthy. Installation
allows two minutes for that work and refuses to restore a version-one binary
after the schema changes.

## Submit a smoke job

```sh
nucleus jobs submit /Users/joey/rust/cell/nucleus/examples/job.smoke.json
nucleus jobs show nucleus-smoke-01
nucleus jobs logs --follow nucleus-smoke-01
```

The checked-in Todo job, schema, and toolset files preserve the immutable
historical `create_todo` contract. They are compatibility fixtures, not a
current Todo canary. Current Todo uses the three closed requester stages
documented in [the integration handoff](docs/annals-todo-handoff.md).

The HTTP API is also available directly over the Unix socket:

```sh
curl --unix-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock" \
  -H 'content-type: application/json' \
  --data-binary \
    @/Users/joey/rust/cell/nucleus/examples/job.smoke.json \
  http://nucleus.local/v1/jobs
```

See [the runtime contract](docs/runtime-contract.md) for exact request and log
examples, and [the integration handoff](docs/annals-todo-handoff.md) for the
deployed Annals and Todo adapter contract.

## Scope

Nucleus owns admission, compatibility checking, execution lifecycle,
cancellation, exact harness-output retention, and retrieval. It reports that an agent
turn completed; it cannot decide that an Annals reconciliation or Todo domain
operation was successful, or authorize a Todo decision. That remains
authoritative in the requester's database.

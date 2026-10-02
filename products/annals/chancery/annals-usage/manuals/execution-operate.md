# Inspect Annals account access and allowance

Annals Usage provides Annals-facing configuration, account, and diagnostic
commands. Nucleus remains the sole execution and credential authority. Use
this feature to read a live account-global allowance, check the configured
reporting path, or perform authorized attended authentication recovery.

This feature does not establish an Annals reconciliation or inbox outcome.
Use `annals-usage.consumption.inspect` for delivery-attributed model tokens and
`nucleus.execution.operate` for general Nucleus service administration.

## Configuration

The installed macOS deployment writes
`$HOME/Library/Application Support/Annals/usage.toml`:

```toml
nucleus = "/absolute/path/to/nucleus"
nucleus_socket = "/absolute/path/to/nucleus.sock"
library = "/absolute/path/to/annals.db"
spool = "/absolute/path/to/spool"
```

An explicit `--config` selects the report, budget, or doctor configuration.
Otherwise selection uses `ANNALS_USAGE_CONFIG`, then `usage.toml` beside a
nonempty `ANNALS_CONFIG`, then the macOS state path under `HOME`. Relative
values resolve from the selected configuration's directory. Unknown keys are
rejected. A valid configuration has no `database` key.

`nucleus_socket` may be omitted to use Nucleus's standard current-user socket.
The executable is used only for delegated login. Report, budget, and doctor
communicate over the socket. The Annals library and spool supply attribution
and delivery context. Annals Usage has no state file of its own.

Annals and Annals Usage deploy together and pin their configurations to the
deployed Nucleus socket. Use `annals.install.operate` for installation,
configuration retention, cutover, and recovery procedures.

## Live allowance

```text
annals-usage budget [--json] [--config PATH]
```

```sh
/Users/joey/.local/bin/annals-usage budget
```

Budget asks Nucleus to make live `account/rateLimits/read` and
`account/usage/read` requests. It reports the account plan, available allowance
windows, used percentages, reset times, and credit fields at one observation
time. Lifetime, peak-day, and latest-day account token activity are contextual
cross-checks; those global activity tokens are not allowance units.

`--json` contains the complete live allowance result and, when available, the
complete activity result alongside observation time and scope. The budget
types and client are exposed by `annals_usage::api`; the CLI produces the same
types. Annals Usage retains none of the responses.

The account response is a coarse account-global snapshot shared with other
Codex work. It does not expose a token denominator behind a subscription
window or identify the delivery that consumed a percentage point. Differences
between repeated snapshots can include concurrent activity and rounding.
Do not use them for per-delivery accounting or call global activity
Annals-specific consumption.

`annals-usage budget` describes live account state at `observedAt`.
`annals-usage report` measures Annals-attributed tokens with explicit coverage.
There is no supported exact conversion between those measurements.

## Diagnostic

```text
annals-usage doctor [--config PATH]
```

```sh
/Users/joey/.local/bin/annals-usage doctor
```

Doctor checks configuration, the configured Nucleus executable, selected
Annals library and spool, Nucleus and Codex versions, strict readiness, and
authenticated account telemetry access. It creates no state. A failure
identifies the current configuration, filesystem, runtime, compatibility, or
account boundary. It does not establish Annals corpus-domain success.

Budget and doctor use a nonblocking canonical-credential request. An
authentication-busy result means a canonical account, refresh, or login
operation owns that boundary. Let it finish. An active job alone does not make
the request busy or indicate invalid credentials. Annals inbox authentication
preflight is a separate operation; its dispatch behavior belongs to
`annals.inbox.operate`.

## Attended login

Help and version flags belong to Annals Usage. The sole login form is:

```sh
/Users/joey/.local/bin/annals-usage login --device-auth
```

It delegates to the configured `nucleus auth login --device-auth` command.
Other login invocations are rejected. Annals communicates with Codex only
through Nucleus.

Use this procedure when authentication recovery is authorized:

1. Prevent new requester work and let active attempts settle.
2. Pause scheduled Annals dispatch if it is active, and wait for its current
   delivery to finish. Preserve the controls already in effect.
3. Run attended login:

   ```sh
   /Users/joey/.local/bin/annals-usage login --device-auth
   ```

4. Run the diagnostic:

   ```sh
   /Users/joey/.local/bin/annals-usage doctor
   ```

5. Confirm account access and strict Nucleus readiness. Stop if either fails.
6. Verify the affected Annals requester. If deliveries failed before the outage
   was detected, keep the pause and use the bounded retry procedure in
   `annals.inbox.operate`. Otherwise resume only the pause established for this
   recovery. Failed account preflight leaves queued jobs unattempted.

Nucleus owns its persistent Codex home and cross-process authentication
coordination. Annals and Annals Usage never read, copy, configure, or retain
credential files. Credential state moves forward and must not be restored by
binary or database rollback. Login changes the Nucleus-owned credential; a
completed login and later service or requester readiness are separate results.

## Authority, access, and limits

Nucleus owns account reads, canonical refresh coordination, authentication,
credentials, and runtime state. Annals Usage owns the live Annals-facing
allowance and diagnostic interpretation. Local commands use the selected
configuration and Nucleus socket; they expose no arbitrary account, credential
path, UI scrape, offline snapshot, remote account, or direct database interface.

Account responses and activity are private information. The companion retains
neither them nor credentials. It creates no second authentication authority.
Unreadable or incompatible authorities fail the live command; the companion
does not return a retained fallback.

No exposed token denominator, monetary invoice unit, Annals-specific allowance
unit, account-data publication latency, wall-clock command latency, historical
retention, atomic snapshot across diagnostic checks, allowance-window
availability, credential-operation wait bound, or service-level objective is
promised. Budget and doctor perform their named live observations on each
invocation; they do not establish broader domain success.

Capability contract version, package and provider release, Nucleus account and
authentication protocol, and Annals library compatibility are distinct axes.
No exact account-response schema guarantee, cross-release support window, or
deprecation period is promised. Nucleus account and authentication reliance is
documented through `nucleus.execution.operate`. Configured Annals library and
spool readiness remains an explicit reliance without a dedicated installed
readiness contract; `annals.corpus.explore` does not supply that promise.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

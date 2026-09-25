# Authentication and account access

Nucleus owns one authoritative managed credential and supplies isolated
credentials to jobs. Use this feature to inspect account access, understand
credential concurrency, or recover authentication through attended login.
Requesters never read, copy, refresh, or lock the canonical credential themselves.

## Interfaces

```sh
nucleus account --wait 0
nucleus auth login --device-auth
nucleus health
```

```text
GET /v1/account?includeUsage=false&waitSeconds=0
GET /v1/health
```

The supported interfaces use the current-user CLI or version-one HTTP over
Nucleus's filesystem-protected Unix socket. There is no TCP endpoint or
application-level authentication. Reading a Chancery entry grants no access or
authority to submit, cancel, mutate requester data, or control the service.

Account reads use bounded credential coordination. `waitSeconds=0` is a
nonblocking try-lock for interactive budget/doctor commands. The Annals inbox
preflight uses up to 30 seconds.
Contention returns `authentication_busy`; credential or account failure returns
`model_auth_unavailable`. `rateLimits` and optional `usage` are the unmodified
results of Codex's `account/rateLimits/read` and `account/usage/read` methods.

An ordinary active job does not by itself block an account read or establish
that the credential is invalid. Health reports authentication readiness;
`authentication_busy` reports credential-operation contention. Account reads
can refresh managed credentials but do not start a model job. Codex owns the
meaning of its returned account payloads: Nucleus promises no independent
accounting units, prices, billing semantics, or account-response objective.

## Credential ownership and concurrency

The macOS service has one authoritative home at
`~/Library/Application Support/Nucleus/codex-home`, with directory mode `0700`.
Its `config.toml` must be a private regular file of at most 64 KiB. It can
contain only `cli_auth_credentials_store = "file"`. `auth.json` must be a
private regular file with mode `0600`.

`nucleus service install --codex-home SOURCE` imports the currently signed-in
`auth.json` after the old daemon stops. The import therefore cannot overlap
an in-flight refresh. Credential state moves only forward and is excluded
from installation rollback. Restoring binaries and the LaunchAgent must not
replace a refreshed token with an earlier consumed credential. This applies
to tokens refreshed by either the old or replacement daemon.

Every job still uses an isolated temporary Codex home. With a static API key,
Nucleus writes only that key into the isolated home and never copies job state
back. With managed ChatGPT authentication, the job home contains no
`auth.json`: after app-server initialization Nucleus supplies the current access
token and account ID through Codex's host-managed in-memory authentication
request, together with optional plan metadata when present. The refresh token
never enters a job home or a harness-output record.

Managed 401 requests use Codex's host refresh callback. Under the canonical
credential lease, Nucleus checks the rejected access-token generation. If
another job already advanced it, Nucleus reuses the current generation.
Otherwise, one managed app-server runs `account/read` against a private staging
copy with proactive refresh enabled. Nucleus checks that the token advanced
without changing accounts. It then fsyncs and atomically promotes the complete
document into the authoritative home before replying.

The supervised refresh continues if its requesting worker is cancelled.
Concurrent 401s therefore share one refresh. They do not create competing
`auth.json` writers or interrupt a write to the authoritative file.

Jobs and account reads hold a shared authentication-session barrier. Attended
`nucleus auth login --device-auth` holds that barrier exclusively, so it waits
for live jobs and account reads before it can replace or revoke the account.
Canonical account reads and refreshes also take the short exclusive credential
mutation lease, but running jobs do not hold that lease for their full turns.
Account reads use a private staging home because Codex can proactively refresh
credentials near expiry. A supervised account operation finishes staged
reconciliation even after requester cancellation or an account deadline.
It atomically promotes a valid same-account generation even if the account
request fails. An incomplete staging write cannot damage the authoritative
file. Attended login also writes only to a private staging home. It promotes
a validated document after successful process completion.
Credential state remains forward-only and only Nucleus's validated atomic
promotion writes the authoritative document. Once imported, Annals and Todo do
not read, write, refresh, or lock Codex credentials themselves.

After HTTP handlers drain, graceful shutdown closes admission to new supervised
account and refresh operations. It repeats cancellation for jobs admitted
during the drain, then waits for started authentication operations to settle
before the daemon exits.

The standard service installer secures its state directory as mode `0700`; the
daemon secures the database and socket as mode `0600`. There is no TCP listener
and no Nucleus authentication protocol in v1. Local user filesystem permissions
are the trust boundary.

## Recovery and effects

Attended login can replace the Nucleus-owned account credential. Prevent new
requester work and let existing job and account sessions finish when they must
remain uninterrupted. Perform attended login, then verify
`nucleus account --wait 0` and `nucleus health`. Restore only the pauses created for that recovery.
`annals-usage login --device-auth` delegates to the same Nucleus operation.

Do not replace credentials merely because an account read reports contention.
Do not restore an older `auth.json` as part of program, service, or database
rollback. A backup of credentials is separate recovery material, not permission
to rewind a consumed or refreshed token. Installation imports credentials only
under the documented service and interrupted-cutover rules in `nucleus.service`.

## Privacy and compatibility

The credential home and any backup contain authentication material. Managed
refresh tokens never enter job homes or harness-output records. Static API-key
jobs receive isolated snapshots without copy-back. Requests and other output
remain private even when the credential exclusions hold.

The exact Codex adapter and account service are external prerequisites; an
installed documentation bundle does not establish their readiness. No
cross-user service, TCP authentication surface, requester-managed canonical
credential, general account-support window, or deprecation interval is promised.

## Related contracts

- Read `chancery show nucleus.jobs`.
- Read `chancery show nucleus.invocation`.
- Read `chancery show nucleus.quota`.
- Read `chancery show nucleus.service`.
- Read `chancery show nucleus.execution.operate`.

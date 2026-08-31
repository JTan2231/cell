# Operate Nucleus agent execution

Nucleus is the per-user execution coordinator for local applications that need
constrained Codex work. It owns admission, one supervised harness attempt per
job, cancellation, authentication serialization, exact harness-output records,
and the durable requester-tool mailbox. It is not a project registry or a
workflow engine, and its terminal job state is never a substitute for an
application's domain result.

## Choose this capability

Use this capability to inspect Nucleus readiness or its authenticated account,
submit an exact version-one job request, inspect or follow a job, request
cancellation, perform attended authentication recovery, or operate the macOS
user service.

Do not route a generic request here merely because it mentions a job, agent, or
model. Todo, Annals, Weaver, and other requesters remain authoritative for the
work that motivated their Nucleus jobs. Ordinary work in the current Codex
session normally needs no Nucleus job at all.

## Supported interfaces

The installed manual is version-matched and does not contact the daemon:

```sh
/Users/joey/.local/bin/nucleus manual
```

Start diagnosis with supported reads:

```sh
/Users/joey/.local/bin/nucleus health
/Users/joey/.local/bin/nucleus service status
/Users/joey/.local/bin/nucleus account --wait 0
/Users/joey/.local/bin/nucleus jobs list --state accepted
/Users/joey/.local/bin/nucleus jobs list --state running
/Users/joey/.local/bin/nucleus jobs list --state waiting-on-requester
/Users/joey/.local/bin/nucleus jobs list --state failed
```

`health` is strict: it prints the readiness document but exits nonzero unless
the daemon is compatible, authenticated, and accepting work. An
`authentication_busy` account result means another credential user owns the
exclusive lease; it does not establish that the credential is invalid.

One exact request is submitted from a file or standard input:

```sh
/Users/joey/.local/bin/nucleus jobs submit <REQUEST_JSON>
/Users/joey/.local/bin/nucleus jobs show <JOB_ID>
/Users/joey/.local/bin/nucleus jobs logs <JOB_ID>
/Users/joey/.local/bin/nucleus jobs logs --follow <JOB_ID>
/Users/joey/.local/bin/nucleus jobs cancel <JOB_ID>
```

The job ID is the idempotency key. Retry an ambiguous submission only with the
byte-equivalent typed request and the same ID. A genuinely new attempt needs a
new ID and the requester must decide that it is safe.

## Effects and authority

Submitting can invoke Codex and consume account allowance. A job receives one
attempt; Nucleus never creates an automatic retry. Dynamic tool calls may
cause requester-owned mutations only after that requester validates and
services them. A successful tool result or Nucleus completion still does not
establish application success.

Cancellation targets one exact job. Repeating the request is idempotent. It
does not remove the job, output history, or a requester mutation already
committed.

Attended login changes the single credential owned beneath Nucleus private
state:

```sh
/Users/joey/.local/bin/nucleus auth login --device-auth
/Users/joey/.local/bin/nucleus account --wait 0
/Users/joey/.local/bin/nucleus health
```

Quiesce requesters before login or service work when active-attempt continuity
matters. Nucleus has no global drain command. A service restart terminates the
daemon; startup marks unfinished attempts `lost`. Service uninstall removes
the user service and installed binaries but deliberately retains state and
logs.

## Success and recovery

For a runtime read, success is the requested supported Nucleus response. For a
direct job, success is admission and the intended runtime observation. When a
requester is involved, separately inspect its database or filesystem result.

If Nucleus cannot connect, inspect service status and
`~/Library/Logs/Nucleus/nucleusd.stderr.log`; do not fall back to an
uncoordinated direct Codex invocation. If a job is waiting on the requester,
inspect the pending mailbox call and requester state rather than inventing a
result. If an attempt is lost, timed out, or failed after a domain commit,
inspect domain state before considering any replacement attempt.

## Privacy

`~/Library/Application Support/Nucleus/` is sensitive. Its database may
contain complete prompts, tool arguments and results, source content, and exact
app-server stdout. Its Codex home contains authentication material. The Unix
socket has no application-level authentication; current-user filesystem
ownership and permissions are the trust boundary.

For backup, migration, deployment, exact harness compatibility, and detailed
recovery ordering, use `nucleus manual` as the current authority.

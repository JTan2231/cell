# CLI contract

```text
clockwork [--json] definition register FILE
clockwork [--json] definition list
clockwork [--json] definition show DEFINITION_DIGEST
clockwork [--json] binding switch KEY DEFINITION_DIGEST
clockwork [--json] binding disable KEY [--select DEFINITION_DIGEST]
clockwork [--json] binding list
clockwork [--json] binding show KEY
clockwork [--json] run KEY
clockwork [--json] history [KEY] [--limit N] [--details]
clockwork [--json] doctor
clockwork [--json] binding halt KEY --code CODE --occurrence ID
clockwork [--json] binding resume KEY INCIDENT_ID
clockwork [--json] incident list [KEY] [--limit N]
clockwork [--json] incident show INCIDENT_ID
clockwork [--json] notification send
clockwork [--json] notification retry INCIDENT_ID
clockwork [--json] migrate --backup ABSOLUTE_NEW_DIRECTORY
```

Program installation is not a Clockwork subcommand. The product deployer takes
separate absolute `--binary` and `--chancery` candidates and uses the latter to
validate the exact staged provider bundle before changing public selectors.

The installed database defaults to
`$HOME/Library/Application Support/Clockwork/clockwork.db`. A hidden absolute
`--state-root` override exists only for controlled tests and isolation. No
command falls back to state in the current directory.

`KEY` is `owner/name`. Each component has at most 63 bytes and starts with a
lowercase ASCII letter. The remaining characters are lowercase letters, digits,
or hyphens. Its unique LaunchAgent label is `org.clockwork.owner.name`.

Every successful command emits a JSON `{"ok":true,"data":...}` envelope;
`--json` selects its compact form. With `--json`, coded failures emit
`{"ok":false,"error":{"code":"...","message":"..."}}` on stderr and
return 1. Without it, failures are human-readable and remain unsuitable as a
machine protocol.

## Definition manifest

`definition register` accepts a regular UTF-8 TOML file of at most 1 MiB.
The file must belong to the current user and must not be a symbolic link.
Group and other users must not have write permission. Clockwork rejects unknown
fields. The version-two shape is:

```toml
schema_version = 2
key = "annals/inbox"
release_id = "64 lowercase hexadecimal characters"
release_root = "/absolute/immutable/release/root"
authority = "current-user-background"
overlap = "skip"
arguments = []
cwd = "/absolute/product/root"
# timeout_seconds = 600  # optional, 1 through 31536000 whole seconds

[schedule]
kind = "interval"
seconds = 300
run_at_load = true

[launch]
kind = "direct"
program = "/absolute/immutable/release/root/bin/annals-inbox-runner"
sha256 = "64 lowercase hexadecimal characters"

[environment]
# SCRUBBED_NON_SECRET_NAME = "literal value"

[output]
stdout = "/absolute/private/log/stdout.log"
stderr = "/absolute/private/log/stderr.log"
```

A local daily calendar schedule is:

```toml
[schedule]
kind = "local-calendar"
hour = 9
minute = 0
run_at_load = false
```

An interpreted launch replaces the direct executable fields with:

```toml
[launch]
kind = "interpreted"
interpreter = "/bin/sh"
interpreter_sha256 = "64 lowercase hexadecimal characters"
script = "/absolute/immutable/release/root/libexec/job-script"
script_sha256 = "64 lowercase hexadecimal characters"
```

All arguments and environment values are literal strings. The child receives
the registered environment plus three Clockwork-owned activation correlation fields. It does not inherit the broker environment. Environment names and values are stored in Clockwork state;
secret-looking names are rejected and callers must not put secrets in any
field. The literal `-c` command-string argument is rejected. The stdout and
stderr paths must be distinct and absolute. Each existing
canonical parent is symlink-free, owner-writable/searchable, and not group- or
world-writable; an existing destination is a private, owner-writable regular
non-symbolic, non-hard-linked file. Clockwork
opens both, rejects equal device/inode identities, and appends or creates them
mode 0600 but never ingests their bodies. Output may not target
the product release or Clockwork's state, broker-log, or LaunchAgent trees.
Version 1 bounds an interval and an optional timeout to 1 through 31,536,000
whole seconds. It requires
`authority = "current-user-background"` and `overlap = "skip"`. A timeout is
otherwise omitted.

Definition registration requires an absolute non-symbolic `release_root` and a
caller-supplied exact 64-lowercase-hex product `release_id`, resolves every
product program or script beneath that root, and verifies artifact digests,
ownership, and permissions before writing a definition row. Opening Clockwork
may first create its private state directories and empty schema-two store. The release root and cwd
must already be canonical, symlink-free current-user-owned directories that
are not group- or world-writable, and the release root's final path component
must equal `release_id`. The manifest file, direct program, and
product script must also be current-user owned. Every launch image is
canonical, symlink-free, non-hard-linked, executable by the current user, and
not group- or world-writable. Direct
programs must carry recognized Mach-O/fat magic; this header check does not
prove that the current host loader can run them. Schema-one interpreted
definitions support only the exact root-owned `/bin/sh` profile, with its hash
recorded separately from the release-local script. Every path ancestor is root- or
current-user-owned and not group- or world-writable. Clockwork pins
but does not recompute a whole release-tree identity. The returned definition
digest is SHA-256 over the canonical JSON encoding of the fully concrete
normalized manifest.
Repeated registration of the identical normalized manifest returns the same
digest; a changed schedule, path, release identity, artifact hash, argument,
environment, working directory, output path, timeout, or policy produces a
distinct immutable digest.

## Binding commands

`binding switch` selects an existing definition digest whose manifest key
exactly matches `KEY`, installs its generated LaunchAgent, and atomically replaces any
prior selection. It refuses an active activation. Success means the database,
plist, and launchd loaded state agree on the new selection. On failure,
Clockwork restores the prior coherent state or, while the projection remains
attributable, durably attempts a disabled state. An unattributable projection
is retained and recovery-gated without mutation.

Binding changes require running the exact current-user-owned binary from a
content-addressed installed Clockwork release; Clockwork checks that binary
against its release manifest before writing its path to a plist. It records the
generated plist's digest and refuses to replace or remove bytes it cannot
attribute to the binding.

`binding disable` prevents new admission and normally waits for an
already-running broker/child to finish naturally through the per-key transition
gate, then boots out the selected schedule and removes its generated plist while
retaining the stable binding identity, definitions, and history. If the broker
has disappeared but its recorded child remains live or cannot be proved absent,
Clockwork retains the running row, rejects disable, restores the prior coherent
binding, and requires a retry after the child is demonstrably gone. It is
idempotent for an already disabled or absent binding; an absent key becomes a
disabled tombstone.

`binding disable KEY --select DEFINITION_DIGEST` performs the same disabled
transition while atomically selecting an already registered same-key definition.
It never loads a schedule or runs the definition. Product deployers use this
recovery form to restore an exact inactive selection without transiently
enabling it.

Disable never starts the product job. A successful switch whose definition has
`run_at_load = true` asks launchd to activate the newly selected key after the
transition gate opens; restoring a prior run-at-load plist during compensation
can make the same request. The product deployer must therefore keep its own
maintenance gate engaged across cutover and recovery. Clockwork does not
promise that launchd will deliver either request. Neither command deletes
product state, release bytes, logs, or Clockwork history.

## Run and private launchd entry

`run KEY` executes the currently selected definition once through the same
broker used by scheduling. It never accepts launch details from the caller.
When the key is already active it returns `activation_busy` with exit 1 and
records a terminal `skipped_overlap` activation without starting a child.

Generated plists use the private `clockwork __launchd KEY` entry point. That
entry is intentionally absent from normal help and is not a public integration
surface. A busy scheduled invocation records the same terminal state but
returns zero so launchd does not infer a scheduler failure.

The broker also uses a hidden `__exec KEY ACTIVATION_ID STATUS_FILE` child
internally. It waits on a parent-only stdin handshake, requires its own PID to
match the already committed running row, and claims the private status marker
before replacing itself with the registered launch image. Explicit pre-exec
failures return only through that marker. Supplying these internal values as an
ordinary caller is not an execution interface.

Once a child starts and Clockwork durably records `exited` (including a nonzero
child exit), `signaled`, or `timed_out`, the broker emits the activation in an
`ok:true` envelope and exits 0. Admission, verification, output-open, spawn,
persistence, manual-busy, and broker failures emit `ok:false` and exit 1. A
pre-start failure after admission is retained as `start_failed` when that
terminal write succeeds. A supervision failure after spawn records the child's
observed terminal state when cleanup is proved; if termination or persistence
cannot be proved, the running row is deliberately left for conservative lost
recovery. These records describe the supervised process. The product records
its own result, such as a sent digest, a processed inbox item, or an accepted
semantic revision.

## Inspection

`definition list` and `definition show DIGEST` expose registered immutable
definitions. `binding list` selects stable bindings and whether each is
enabled. `binding show KEY` returns the recorded binding and selected
definition digest; it does not query launchd or expose the internal plist
digest. An absent binding returns
`binding_not_found`; disable is idempotent and creates a disabled tombstone for
an absent key. Paths, non-secret environment, hashes, and identifiers are private operational
metadata.

`history [KEY]` returns newest activations first, optionally restricted to one
key. Definition/binding lists and history default to 20 rows, accept positive
`--limit`, and return output-version-two pages (`items`, `has_more`). Increase
`--limit` for more. History rows contain activation ID, key, trigger, timestamps,
state, exit code, signal and failure detail. `history --details` includes the
complete records with definition digest and process IDs. It contains no captured output body and no
domain-success field.

## Doctor

`doctor` opens the schema-two store, enforces private owned state paths, runs
SQLite `quick_check`, resolves the current Clockwork executable, and verifies
`/bin/launchctl` is present. It also marks a retained `running` activation
`lost` when both its recorded broker and any recorded child are demonstrably
absent, reports the count, and lists pending binding-transition journals.
Doctor does not repair those journals because `binding switch` may restore a
run-at-load service while `binding disable` resolves directly to an inactive
selection. Invoke the intended operation for that exact key under the
product's maintenance gate. Doctor does not execute a job, switch a binding,
inspect product state, interpret domain success, prove future timer delivery,
or establish a launchd availability SLA.


## Failure policy and explicit continuation

Manifest schema two adds a product-owned failure configuration:

```toml
[failure]
on_abend = "halt-until-approved"
# email_cli = "/Users/operator/.local/bin/email"
```

Omission means `halt-until-approved`. The only exception is
`continue-next-activation`, which retains the abend and permits the next timer
activation. It does not retry the failed occurrence. The product declares and
explains any exception. `email_cli` selects one absolute installed Email
wrapper; omission selects `$HOME/.local/bin/email` for the current Clockwork
user. This is a separately contracted Email transport, not an extension of the
attested product launch image.

A schema-two startup failure, nonzero direct-child exit, signal, timeout,
lost activation, or broker supervision failure applies the selected policy.
The product owns the interpretation of expected empty, waiting, and deferred
outcomes and returns zero for those outcomes. An overlap is normal and never
halts scheduling. Zero remains runtime evidence, not domain success.

A product can report a terminal domain failure while its supervised process
still runs, including after it durably recorded that failure:

```rust
clockwork::api::report_abend("model_failed", "job/immutable-job-id")?;
```

The helper returns `false` outside Clockwork and rejects a partial context. A
schema-two child receives `CLOCKWORK_BROKER_PATH`, `CLOCKWORK_STATE_ROOT`, and
`CLOCKWORK_ACTIVATION_ID` in addition to its registered environment. Preserve
those values across a product-owned environment scrub. Registered environment
names beginning `CLOCKWORK_` are reserved. The helper invokes the exact broker
with `abend ACTIVATION_ID --code CODE --occurrence ID`; it does not access
Clockwork storage directly. The activation must still be running.

A report contains only a bounded machine code (64 bytes) and immutable product
occurrence ID (256 bytes). Both permit ASCII letters, digits, `-`, `_`, `/`,
`.`, and `:`. Do not supply source text, an email body, a credential, or an
unbounded error message. Clockwork retains each `(key, occurrence)` once.
Reporting that same failed occurrence after approval does not create a new
halt. A distinct failed product attempt needs a distinct occurrence ID. Stop
claiming successor work as soon as an abend is encountered; a report cannot
cancel already admitted product work or make product commits atomic with the
Clockwork incident.

A default-policy abend durably opens one incident and closes admission for its
stable key. Terminal runtime evidence and its incident commit together. A
product report commits the incident before returning. The halt is independent
of `enabled`, selected definition, product maintenance, and deployment.
Switching, disabling, compensation, restart, and reinstall never clear it.
The loaded timer may still invoke the broker, which starts no product child.
A halted launchd invocation returns its binding receipt without another
activation or incident. A manual run returns `binding_halted`.

Inspect or explicitly approve continuation:

```sh
clockwork binding show owner/name
clockwork incident list owner/name --limit 20
clockwork incident show INCIDENT_ID
clockwork binding resume owner/name INCIDENT_ID
```

`halted_incident` identifies the current incident. `failure_policy_active`
indicates a selected schema-two definition. Incident timestamps are whole Unix
seconds; notification timestamps and attempts describe only that incident's
email submission attempts. `resume` requires the exact open incident, no
running activation, and no pending binding transition. It records approval and
opens future admission. It does not enable a disabled binding, activate a timer,
retry a failed product occurrence, or undo committed work. Only an explicit
user go-ahead authorizes this command; product installers must not invoke it.

When a product already owns a failure pause, transfer that gate under product
maintenance before permitting scheduled work:

```sh
clockwork binding halt owner/name --code legacy_failure --occurrence legacy/ID
```

This idempotently retains the failure incident without enabling or selecting
work. An absent key becomes a disabled tombstone. After the durable receipt,
the product may remove only its failure-owned scheduling gate. Keep product
failure evidence, item recovery state, user pauses, and maintenance holds.
Configuration remains product-owned; future scheduling failure enforcement is
Clockwork-owned. The migration does not infer old pauses by reading product
files or scanning old activation history.

## Pause notification

Each halt carries standing authority for one plain-text notification to Email's
fixed personal recipient. The subject and body identify the binding, incident,
failure code, occurrence, activation, halt time, and inspection/continuation
commands. These metadata are disclosed to Email, Resend, and Gmail. Clockwork
retains no product output, email response body, or credential. Email loads its
own credential and owns bounded HTTP transport; exit zero establishes provider
acceptance, not inbox delivery.

Clockwork retains a pending notification in the same transaction as the halt.
Every scheduled or manual broker visit attempts at most one due notification
before the product gate, and one after the activation outcome. Attempts use a
private transport lock, a fixed payload and idempotency key, a 120-second
process bound, and at least five minutes between attempts for one incident.
The halt remains closed when transport or notification bookkeeping fails.
Other timers can deliver an alert for a disabled or halted product. There is
no extra daemon or timer; if no broker is invoked, pending mail waits.

Run `clockwork notification send` to attempt one due notification independently
of product scheduling. Read its incident to distinguish `pending`, `accepted`,
and `uncertain`. An interrupted or failed transport may already have been
accepted. After 23 hours from the first invocation, or a backwards clock jump
before that invocation, Clockwork makes no automatic further attempt and
retains `uncertain`, leaving margin before Resend's 24-hour deduplication limit.
Inspect provider acceptance before issuing
`clockwork notification retry INCIDENT_ID`. That command explicitly approves
possible duplication, creates a new idempotency generation, and attempts the
same retained incident payload. It never clears the scheduling halt.

## Storage and definition upgrade

Clockwork 0.5 uses SQLite schema two and requires an explicit
`clockwork migrate --backup /absolute/new-backup-directory` for schema one.
Quiesce all Clockwork commands and product schedules first; recover running
rows and pending binding transitions with the old binary. Migration takes a
schema gate, refuses retained running rows, checkpoints SQLite, retains a
private database-plus-sidecar backup, and applies the transactional schema
change. It does not change definitions, selections, activation history, timers,
or product pauses. Program deployment never performs this migration.

Schema-one definitions keep their original digest and legacy failure behavior.
They do not acquire the new policy merely because the database migrated.
Register each product's schema-two definition and select it under its
maintenance gate, preserving whether the binding was disabled and importing
any existing failure halt. `failure_policy_active: false` makes this rollout
gap visible. Generated plists also pin an exact Clockwork binary; refresh each
supported product binding before releasing maintenance. Retired definitions
remain available as historical evidence.

After migration, an old Clockwork binary cannot open the new store. Database
rollback requires the retained schema-one database and sidecars, compatible
Clockwork and product releases, their prior definitions/plists, and quiescence.
It must preserve all newer halt evidence; do not restore a pre-halt backup and
silently resume work. Keep the failed store and incident export for recovery.

The hidden `--state-root` test override uses `STATE_ROOT/email` as its default
Email double, preventing isolated fixtures from selecting the real account.
An explicit `failure.email_cli` still selects the caller's authorized wrapper.
The canonical ordinary state path used by a product report retains the normal
installed layout and Email default.

## EMT notification integration

~~~sh
clockwork incident feed --after CURSOR --limit 100
clockwork notification emt --receiving-domain DOMAIN
clockwork notification emt --disable
clockwork notification show INCIDENT_ID
clockwork notification claim INCIDENT_ID --delivery-id UUID
~~~

Feed returns items, next_cursor and has_more in insertion order. Show returns
the basic payload and ownership and can materialize the incident reply route.
Claim is for an already retained EMT email and refuses a started basic send or
a different delivery owner. Disabling new EMT routing preserves existing
routes and claims. These operations never resume a schedule.

See the installed schedule-operation manual for grace timing, transport
ownership, cursor recovery and the required broker cutover.

## Coordinated broker deployment

Coordinated `./deploy.sh clockwork` also captures the complete Clockwork binding
inventory before maintenance. It disables those bindings while retaining their
selections and failure halts. Product adapters prepare their new definitions
under their own holds. After all holds are released, Clockwork restores each
previously enabled binding through the selected broker. This rewrites every
enabled generated plist with the current immutable Clockwork executable.
Previously disabled bindings stay disabled. This phase precedes EMT activation.
An interrupted deployment retains its original inventory and re-establishes
suspension before recovery; it does not infer intent from temporary disablement.

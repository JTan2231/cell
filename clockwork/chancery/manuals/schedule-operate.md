# Register and operate Clockwork schedules

Clockwork schedules non-agent product runners for the current user. It has no
daemon. launchd starts a short-lived Clockwork broker with one stable
`owner/name` key. Clockwork selects an immutable definition, enforces per-key
overlap, and verifies the registered top-level image. It then starts one child
process group, waits, and records the runtime result.

Clockwork owns this mechanical boundary. The product still owns its release,
durable work, idempotency, locks, retries, secrets, output files, recovery, and
domain-success rule. A Clockwork `exited` result, including exit zero, is not
proof that product work succeeded.

## Register an immutable definition

Prepare a regular UTF-8 TOML file of at most 1 MiB. The file must belong to the
current user and must not be a symbolic link. Group and other users must not
have write permission. Clockwork rejects unknown fields. A direct launch uses
this version-two shape:

```toml
schema_version = 2
key = "owner/name"
release_id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
release_root = "/absolute/product/install/releases/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
authority = "current-user-background"
overlap = "skip"
arguments = ["literal", "arguments"]
cwd = "/absolute/product/root"
# timeout_seconds = 600 # optional; 1 through 31536000

[schedule]
kind = "interval"
seconds = 300
run_at_load = true

[launch]
kind = "direct"
program = "/absolute/product/install/releases/0123.../bin/product-runner"
sha256 = "64-lowercase-hex-sha256"

[environment]
HOME = "/Users/operator"

[output]
stdout = "/absolute/private/product/logs/scheduled.out.log"
stderr = "/absolute/private/product/logs/scheduled.err.log"
```

For one local daily trigger use:

```toml
[schedule]
kind = "local-calendar"
hour = 9
minute = 0
run_at_load = false
```

For an interpreted runner use exact separately hashed interpreter and script
images:

```toml
[launch]
kind = "interpreted"
interpreter = "/bin/sh"
interpreter_sha256 = "64-lowercase-hex-sha256"
script = "/absolute/product/install/releases/0123.../libexec/product-job"
script_sha256 = "64-lowercase-hex-sha256"
```

Clockwork invokes the interpreter directly with the script as data. It does
not use the script shebang, and the literal `-c` command-string argument is
rejected. All arguments and
environment values are literal, and the registered environment replaces the broker environment. Schema two adds the three reserved activation correlation fields described below. Secret-looking environment names are rejected; never
place a credential in any field because definitions are durable private
metadata, not a secret store.

The two `[output]` destinations are distinct absolute product-owned files.
Their existing canonical parent directories must be symlink-free,
owner-writable/searchable, and not group- or world-writable. An existing
destination must be a private, owner-writable regular, non-symbolic,
non-hard-linked file. Clockwork opens both destinations, verifies
their device/inode identities differ, and appends or creates them mode 0600,
then streams the child directly there without ingesting the body. These are separate
from the broker-only logs named by the generated LaunchAgent under
`~/Library/Logs/Clockwork`. They may not target product release bytes or
Clockwork-owned state, broker-log, or LaunchAgent trees.

The key components are each at most 63 bytes, begin with a lowercase ASCII
letter, and continue with lowercase letters, digits, or hyphens. The label is
`org.clockwork.owner.name`. The release root must be an exact absolute
non-symbolic product release whose final path component is `release_id`, its caller-supplied
product-published lowercase 64-hex content identity. Clockwork pins that value
but does not recompute or attest a whole-tree hash. The manifest, release root,
cwd, program, and script are current-user-owned. Release root and cwd are
already canonical, symlink-free directories that are not group- or
world-writable. Every ancestor is root- or current-user-owned and not group- or
world-writable. The direct program and interpreted script are beneath that
release. Direct programs carry recognized Mach-O/fat magic; that discriminator
does not prove the current host loader can run the image. Schema-one
interpreted definitions support only exact root-owned `/bin/sh`. Every launch
image is canonical, regular, non-symbolic, non-hard-linked, executable by the
current user, not group- or
world-writable, and matches its registered SHA-256.

Register without changing the active schedule:

```sh
/Users/joey/.local/bin/clockwork definition register /absolute/definition.toml
```

Registration is idempotent for the exact normalized definition. It returns the
SHA-256 digest of canonical JSON for the fully concrete normalized manifest and
never edits an older definition. It performs no child execution or launchd
cutover.

## Switch or disable a binding

Switch only after the product has staged and validated its exact candidate
release and recorded the prior Clockwork binding and product rollback basis:

```sh
/Users/joey/.local/bin/clockwork binding switch owner/name DEFINITION_DIGEST
```

Clockwork takes the per-key transition lock, refuses an active activation,
captures prior binding/plist/loaded state, boots out the prior generated agent,
stages the replacement plist atomically, commits the selected definition and
generated-plist digest, and
bootstraps it into the current user's GUI domain. A successful return means all
three views agree. It verifies recorded plist bytes before replacement or
removal. On failure it compensates to the captured prior coherent state. If
that cannot be proved while the current state still matches the recorded prior
or candidate projection, it durably records disable intent and tries to leave
the binding visibly disabled. An unattributable projection is retained without
mutation, and Clockwork reports whether disabled state could be proved. Do not
create an old-and-new dual schedule as recovery.

Before changing an existing binding or launchd projection, Clockwork fsyncs a
private per-key transition journal. It contains the operation, target definition,
and exact prior binding, plist, and loaded state. For a switch, it also contains
the exact candidate definition and plist bytes. Recovery refuses a current
binding or plist matching neither recorded projection. If the broker is
abruptly terminated, `doctor` reports the pending key but deliberately does
not choose a repair effect. Under the product's maintenance gate, rerun
`binding switch` to restore the prior state before cutover, or use `binding
disable` to durably replace it with a disable intent and consume that journal
directly into the requested inactive
selection without loading either generation.

If journal unlink succeeds but its directory sync fails, Clockwork leaves the
coherent binding projection in place, reports commit durability as uncertain,
and does not attempt an unjournaled rollback.
Disabling a wholly absent coherent key is a single atomic disabled-tombstone
write and has no external state requiring a journal.

Disable through the same guarded lifecycle:

```sh
/Users/joey/.local/bin/clockwork binding disable owner/name
```

Disable first prevents new admission and normally waits under the key lock for
an already-running broker/child to finish naturally, then removes the loaded
schedule and generated plist. If the broker disappeared while its recorded
child remains live or cannot be proved absent, disable rejects, restores the
prior coherent binding, and must be retried after demonstrable child exit. It
retains the stable binding, definitions, and activation history. Disable does
not start the product job. Switching or compensating a
definition with run-at-load enabled asks launchd to activate the key after the
transition gate opens, so the producer must retain its maintenance gate until
cutover or rollback has committed. Clockwork does not promise delivery of that
request. Neither operation deletes product state, logs, or releases.

To restore an exact registered selection while keeping the binding disabled,
use the recovery form:

```sh
/Users/joey/.local/bin/clockwork binding disable owner/name --select DEFINITION_DIGEST
```

This does not install or load a LaunchAgent and does not run the product.

## Run and inspect

Run the currently selected definition once through the same broker:

```sh
/Users/joey/.local/bin/clockwork run owner/name
```

The command accepts no executable, arguments, environment, cwd, timeout, or
policy. Every busy invocation records `skipped_overlap` and starts no child. A
busy manual run returns `activation_busy` and exit 1; the private launchd entry
returns success to launchd for a normal overlap. Neither path retries.

Before product execution, the broker starts its exact installed Clockwork
binary as a blocked process-group leader, records that PID, and releases it
through a one-byte parent pipe. EOF before release exits without product work;
after release the gate requires its PID to match the committed running
activation and replaces itself with the registered image. The hidden
key-plus-activation gate is not a caller-facing command surface.

Inspect current definitions, bindings, and history:

```sh
/Users/joey/.local/bin/clockwork definition list
/Users/joey/.local/bin/clockwork definition show DEFINITION_DIGEST
/Users/joey/.local/bin/clockwork binding list
/Users/joey/.local/bin/clockwork binding show owner/name
/Users/joey/.local/bin/clockwork history owner/name --limit 20
/Users/joey/.local/bin/clockwork doctor
```

History states are `running`, `start_failed`, `exited`, `signaled`,
`timed_out`, `skipped_overlap`, and `lost`. One activation is at most one child
and has no attempt children. Clockwork stores the direct process result but no
output body. Once a child starts and Clockwork durably records `exited`,
`signaled`, or `timed_out`, the broker emits `ok:true` and exits 0 even for a
nonzero child exit. Admission, validation, output-open, spawn, persistence,
manual-busy, and broker failures emit `ok:false` and exit 1; a post-admission
start failure is recorded `start_failed` when that terminal write succeeds,
otherwise the row remains conservatively `running`. A prior `running` row becomes
`lost` only after its broker and any child are both demonstrably absent.

Doctor initializes only an empty unversioned local schema-two store, refuses a
foreign or unsupported schema, enforces private state paths, runs SQLite
`quick_check`, resolves the current executable, and checks
that `/bin/launchctl` exists. It also marks a retained `running` activation
`lost` only when the recorded broker and any recorded child are demonstrably
absent. It does not execute, switch, inspect product state, infer domain
success, or prove the next launchd delivery.

## Timer and executable limits

The generated plist contains the exact content-addressed installed Clockwork
binary after checking it against its release manifest, the stable key, trigger,
`HOME`, and Clockwork broker log paths. It never contains the
product program, arguments, or secret environment. launchd delivery depends on
the user's GUI login domain, sleep/wake and timer semantics, clock and time-zone
changes, TCC and filesystem access, and resource pressure. There is no maximum
start delay, catch-up count, fairness, or availability promise.

Clockwork's executable guarantee covers only the registered top-level program
or interpreter and script at verification time. It does not attest transitive
libraries, later-opened configuration, subprocesses, network peers, same-user
tampering after verification, or product meaning.

Stable `owner/name` keys are operational namespaces, not authentication among
processes running as the same user. Clockwork serializes each binding mutation
internally, but v1 has no compare-and-swap precondition covering a product's
earlier ownership inspection. Product lifecycle tools must serialize their own
use; concurrent direct same-user mutation of the same key is unsupported and
may cause refusal or recovery gating.

Stop rather than broadening the definition when the product needs arbitrary
commands, a secret, inherited environment, a mutable selector, workflow
dependencies, retry/backoff, output capture, or a system service. Those require
a different reviewed contract.

## Output selection

Registration and binding changes return definition or binding receipts. Run
returns one direct-child runtime outcome. Definition lists, binding lists, and
history use output version 2 with `items` and `has_more`. They return up to 20
rows by default. To read more, set `--limit` to a larger positive integer.

History returns activation ID, key, trigger, timestamps, state, exit code,
signal, and failure detail. `--details` adds the definition digest and process
IDs. Definition and binding show return the full selected metadata.


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

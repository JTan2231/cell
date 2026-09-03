# Register and operate Clockwork schedules

Clockwork is the current-user scheduled-activation broker for non-agent product
runners. It has no daemon. launchd starts a short-lived Clockwork broker with
one stable `owner/name` key; Clockwork selects an immutable definition,
enforces per-key overlap, verifies the registered top-level image, directly
spawns one child process group, waits, and records a runtime result.

Clockwork owns this mechanical boundary. The product still owns its release,
durable work, idempotency, locks, retries, secrets, output files, recovery, and
domain-success rule. A Clockwork `exited` result, including exit zero, is not
proof that product work succeeded.

## Register an immutable definition

Prepare a current-user-owned, non-group/world-writable regular non-symbolic
UTF-8 TOML file of at most 1 MiB. Unknown fields are rejected. A direct launch
has this version-one shape:

```toml
schema_version = 1
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
environment values are literal, and the exact registered environment replaces
the broker environment. Secret-looking environment names are rejected; never
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

For an existing binding or launchd projection, Clockwork fsyncs the operation, target definition, exact prior binding/plist/
loaded-state observation, and exact candidate definition and plist bytes for a switch to a
private per-key transition journal before mutation. Recovery refuses a current
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

Doctor initializes only an empty unversioned local schema-one store, refuses a
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

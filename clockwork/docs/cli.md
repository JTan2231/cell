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
clockwork [--json] history [KEY] [--limit N]
clockwork [--json] doctor
```

Program installation is not a Clockwork subcommand. The product deployer takes
separate absolute `--binary` and `--chancery` candidates and uses the latter to
validate the exact staged provider bundle before changing public selectors.

The installed database defaults to
`$HOME/Library/Application Support/Clockwork/clockwork.db`. A hidden absolute
`--state-root` override exists only for controlled tests and isolation. No
command falls back to state in the current directory.

`KEY` is `owner/name`; both components are at most 63 bytes, begin with a
lowercase ASCII letter, and then contain only lowercase letters, digits, or hyphens. Its collision-free
LaunchAgent label is `org.clockwork.owner.name`.

Every successful command emits a JSON `{"ok":true,"data":...}` envelope;
`--json` selects its compact form. With `--json`, coded failures emit
`{"ok":false,"error":{"code":"...","message":"..."}}` on stderr and
return 1. Without it, failures are human-readable and remain unsuitable as a
machine protocol.

## Definition manifest

`definition register` accepts a current-user-owned, non-group/world-writable
regular non-symbolic UTF-8 TOML file of at most 1 MiB. Unknown fields are
rejected. The version-one shape is:

```toml
schema_version = 1
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
exactly the registered environment rather than inheriting Clockwork's process
environment. Environment names and values are stored in Clockwork state;
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
may first create its private state directories and empty schema-one store. The release root and cwd
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
recovery. These are runtime observations. They do not claim that a digest was
sent, an inbox was drained, a semantic change was correct, or any other product
goal succeeded.

## Inspection

`definition list` and `definition show DIGEST` expose registered immutable
definitions. `binding list` returns all stable bindings and whether each is
enabled. `binding show KEY` returns the recorded binding and selected
definition digest; it does not query launchd or expose the internal plist
digest. An absent binding returns
`binding_not_found`; disable is idempotent and creates a disabled tombstone for
an absent key. Paths, non-secret environment, hashes, and identifiers are private operational
metadata.

`history [KEY]` returns newest activations first, optionally restricted to one
key. `--limit` defaults to 100 and accepts 1 through 1000. Each row includes
activation and definition identity, source (`manual` or `launchd`), whole Unix
second timestamps, state, broker and direct-child PID where known, exit code,
signal, and applicable detail. It contains no captured output body and no
domain-success field.

## Doctor

`doctor` opens the schema-one store, enforces private owned state paths, runs
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

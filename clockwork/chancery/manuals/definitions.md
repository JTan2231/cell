# Immutable activation definitions

Use this feature to understand and register one exact non-agent product runner.
A definition owns the complete immutable schedule and launch context. Runtime
callers select its stable binding key; they cannot supply process context.
Clockwork owns definition identity and local artifact admission. The product
owns its published release, work, secrets, logs, retries, and domain success.

## Interfaces

```text
clockwork [--json] definition register FILE
clockwork [--json] definition list [--limit N]
clockwork [--json] definition show DEFINITION_DIGEST
```

Commands print plain text by default. `--json` selects the existing compact
`ok:true` / `data` JSON envelope. With that flag, coded `ok:false` / `error`
failures go to stderr with exit one. Other failures are human-readable on
stderr. Machine callers must pass `--json`.

Register validates the complete definition and direct launch images before
storing a definition. It can prepare private state and initialize an empty
unversioned store as schema two. It performs no execution or binding cutover.
Invalid fields, paths, permissions, hashes, or policies reject registration.
Correct the product manifest or artifacts; do not bypass the checks.

List and show return stored immutable metadata, including paths, literal
arguments, non-secret environment, hashes, and schedule. List uses output
version two with `items` and `has_more`, defaults to 20 rows, and accepts a
positive `--limit`. Increase the limit to read a larger prefix. Reads select
retained definitions at invocation; they do not prove future artifact validity.

## Manifest and identity

The required `clockwork.incidents` contract supplies schema-two `[failure]`
fields, default policy, explicit exception, Email-wrapper selection, and
reserved activation correlation fields. Resolve this feature to include that
complete input meaning.

`KEY` is `owner/name`. Each component has at most 63 bytes, begins with a
lowercase ASCII letter, and continues with lowercase letters, digits, or
hyphens. Its collision-free LaunchAgent label is `org.clockwork.owner.name`.

`definition register` accepts a regular UTF-8 TOML file of at most 1 MiB.
The file must belong to the current user and must not be a symbolic link.
Group and other users must not have write permission. Clockwork rejects unknown
fields. The version-two shape is:

```toml
schema_version = 2
key = "annals/inbox"
release_id = "01234567-89ab-7cde-8fab-0123456789ab"
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

Arguments and environment values are literal strings. The registered environment
replaces the broker environment. A schema-two child additionally receives the
three reserved activation correlation fields. Names and values are stored in state;
secret-looking names are rejected and no field may contain a secret. The
literal `-c` command-string argument is rejected.

Output paths are distinct and absolute. Their existing canonical parents are
symlink-free, owner-writable/searchable, and not group- or world-writable.
Existing destinations must be private, owner-writable regular files without
symbolic or hard links. Outputs cannot target the product release or Clockwork's
state, broker-log, or LaunchAgent trees. Runtime opening and append behavior
belong to `clockwork.activations`; Clockwork never ingests the bodies.

Both supported manifest schemas bound interval and optional timeout values to
1 through 31,536,000 whole seconds. Local-calendar schedules use hour 0 through
23 and minute 0 through 59. Both schemas require
`authority = "current-user-background"` and `overlap = "skip"`. A timeout can
be omitted.

Definition registration requires an absolute non-symbolic `release_root` and a
caller-supplied exact product `release_id` (canonical UUID or retained
64-lowercase-hex release ID), resolves every
product program or script beneath that root, and verifies artifact digests,
ownership, and permissions before writing a definition row. Opening Clockwork
may first create its private state directories and empty schema-two store.

The release root and cwd must already be canonical, symlink-free
current-user-owned directories that
are not group- or world-writable, and the release root's final path component
must equal `release_id`. The manifest file, direct program, and
product script must also be current-user owned. Every launch image is
canonical, symlink-free, non-hard-linked, executable by the current user, and
not group- or world-writable.

Direct programs must carry recognized Mach-O/fat magic; this header check does not
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

## Verification and trust limits

An executable means the registered top-level launch image, not a command
string or the whole process tree. Clockwork verifies each pinned direct image
at registration and again immediately before execution. The product publishes
the whole release identity. Clockwork does not attest transitive libraries,
later-opened configuration, subprocesses, network peers, same-user tampering
after verification, or product meaning.

Paths must already be concrete and canonical. Mutable release selectors,
PATH lookup, relative or symbolic launch paths, shell strings, interpolation,
globbing, inherited environment, and implicit shebang selection are unsupported.
Stable keys are operational namespaces, not authentication between same-user
processes. Direct SQLite access is unsupported.

## Compatibility and related features

Schema-one and schema-two manifests, SQLite schema, provider release, product
release identity, and definition digest are separate compatibility axes.
Existing definitions keep their content and identity. Changed registered
meaning requires a new definition and explicit binding selection. The versioned Clockwork failure-check contract applies its delay to schema-two
failures without rewriting retained definitions. A database migration alone
does not add failure enforcement to schema-one definitions.
No retention horizon, deprecation interval, or cross-release migration window
is promised.

`clockwork::api` owns the manifest and definition types, codecs, and typed
client for one explicitly selected Clockwork executable. Structural decoding
alone does not perform registration's local-artifact checks.

Read `clockwork.bindings` for selection, `clockwork.incidents` for schema-two
failure fields and correlation, and `clockwork.activations` for output opening
and direct supervision. Use `clockwork.schedule.operate` for the procedure.

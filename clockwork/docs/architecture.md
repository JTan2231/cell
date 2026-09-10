# Architecture

Clockwork manages scheduled activation. Each product owns the work it performs.

```text
product release
  -> register immutable definition
  -> switch stable owner/name binding
  -> generated user LaunchAgent contains only the stable key
  -> short-lived Clockwork broker pins the selected definition
  -> verify registered top-level launch image
  -> admit or skip one activation
  -> spawn and supervise one direct child process
  -> record runtime outcome
```

There is no resident Clockwork daemon. Each scheduled or manual invocation is
a short-lived broker process backed by private SQLite state and a per-binding
admission lock. launchd owns timer delivery and process invocation; Clockwork
owns the decision about which immutable definition the stable key currently
selects and the direct child's runtime record.

Program deployment is a separate packaging boundary. It stages Clockwork's
binary, Rust installer, and provider as one content-addressed release, then
requires an explicitly supplied candidate Chancery reader to validate the
exact staged provider copy before changing either public selector. That
direct installation does not open runtime state or mutate a product binding.
The coordinated adapter separately suspends the complete binding inventory and
refreshes enabled broker plists after all product holds are released. It keeps
disabled intent, immutable product selection, and recorded failure halts.

## Authority

Clockwork owns:

- definition identity and immutability;
- stable binding selection and generated `org.clockwork.*` LaunchAgents;
- per-key overlap admission, declared failure-policy enforcement, durable halt incidents, and pause-email state;
- verification and direct supervision of the registered launch image; and
- activation identity, timestamps, process identity, and terminal runtime
  state.

The registering product owns the artifact bytes it publishes, the work done by
the child, durable checkpoints, product locks, idempotency, retries, secrets,
logs, recovery, and the meaning of domain success. launchd owns timer delivery
under the current macOS user session. Clockwork does not turn a zero process
exit into proof that a product accomplished its goal.

## Executable contract

An executable is the registered **top-level launch image**. It identifies the
program Clockwork starts, without describing a whole process tree.

A direct definition pins an executable program beneath an immutable release
directory. The program must belong to the current user and have recognized
Mach-O or fat-binary magic. The definition records its lowercase SHA-256 digest,
literal arguments, absolute working directory, and scrubbed environment without
secrets.

An interpreted definition pins schema one's only interpreter profile: the exact
root-owned `/bin/sh` bytes. It also pins an absolute regular executable script
beneath the release, owned by the current user. Each image has its own digest.
Clockwork invokes `/bin/sh` directly with the script as its first operand. It
does not use the shebang and rejects the literal `-c` command-string argument.

At registration and again before spawn, the pinned artifacts must be regular,
non-symbolic, non-hard-linked, executable by the current user, and not group-
or world-writable. The Mach-O magic check is a closed format discriminator,
not a complete loader or host-architecture preflight; an otherwise admitted
image may still fail at spawn and is recorded as `start_failed` when the
terminal state write succeeds.

The manifest's release root is an absolute non-symbolic directory whose final
path component equals the product's caller-supplied exact 64-lowercase-hex
content release ID. Clockwork pins that identity but does not recompute or
attest a whole release-tree digest. The product deployer must render the
canonical exact release path before registration; `current`, `latest`, PATH
lookup, working-directory lookup, glob expansion, variable interpolation, and
shell parsing are not accepted.

Every path ancestor must be a root- or current-user-owned directory that is not
group- or world-writable. Product output targets may not overlap the immutable
release or Clockwork-owned state, broker-log, or LaunchAgent trees.

This guarantee ends at the top-level images named by the definition. Clockwork
does not attest dynamically loaded libraries, configuration opened later,
subprocess executables, network peers, same-user tampering after verification,
or product meaning. A product that needs stronger whole-tree provenance must
enforce it in its own release and runner contract.

The `owner/name` key is an operational namespace, not authentication between
processes running as the same user. Clockwork serializes its own mutation of a
key, but v1 exposes no compare-and-swap precondition for a product deployer's
earlier ownership inspection. Product deploy/uninstall transactions must
serialize their own use; concurrent direct same-user binding mutation is
unsupported and can cause refusal or recovery gating.

## Definition and binding lifecycle

Registration validates a complete manifest whose paths are already canonical,
verifies artifact hashes and permissions, and writes a new immutable
definition. A definition digest identifies those exact bytes and launch semantics.
Definitions are never edited in place.

A binding is the stable product key `owner/name`. Switching a binding is a
transactional operational change under a per-key management lock and an
exclusive per-key transition gate:

1. refuse an active activation;
2. capture the prior selected definition, plist bytes, and loaded state;
3. boot out the prior generated LaunchAgent when present;
4. stage the new plist atomically;
5. commit the selected definition and generated-plist digest; and
6. bootstrap the replacement LaunchAgent.

If cutover fails, Clockwork restores the prior database selection, plist, and
loaded state. If that restoration fails while the current projection still
matches recorded prior or candidate state, Clockwork first persists a disable
intent and attempts fail-disabled cleanup. An unrecognized projection is left
untouched and recovery-gated; Clockwork reports explicitly when disabled state
cannot be established.

It refuses to replace or remove a plist that does not match the binding's
recorded digest. It never leaves an intentional old-and-new dual schedule.
Disabling a binding first prevents new admission, waits for exclusive
ownership of the transition gate while any already-running broker/child
finishes naturally, then boots out the generated agent and removes its plist.

If a broker has disappeared while its recorded child remains live or cannot be
proved absent, the shared gate no longer provides a wait handle: disable
rejects, restores the prior coherent binding, and must be retried after
demonstrable child exit. It retains definition and activation history.

For a transition involving an existing binding or launchd projection,
Clockwork fsyncs a private per-key transition journal containing the
operation, target definition, exact prior binding/plist/loaded-state
observation, and the exact candidate definition and plist bytes for a switch
before the first mutation. Recovery touches a current plist or binding only
when it matches the journal's prior or candidate projection; an unattributable
projection is retained and reported.

A later switch for that key restores a switch journal left by an abruptly
terminated broker before attempting the requested change. A later disable
first durably replaces either journal with its requested disable intent, then
resolves it directly into a disabled selection without loading a schedule.
Doctor reports pending transition keys without choosing either repair effect.

If the journal unlink succeeds but its directory sync fails, Clockwork retains
the already coherent binding projection, reports commit durability as
uncertain, and does not begin an unjournaled rollback. Disabling a wholly
absent coherent key instead creates one disabled SQLite tombstone atomically;
there is no external projection to coordinate.

Bootstrapping a definition with run-at-load enabled may make launchd request
an activation once the transition gate opens. The same is true when recovery
re-bootstraps a prior run-at-load definition. Product deployment therefore
keeps its own domain-maintenance gate engaged until either cutover or rollback
has committed; Clockwork neither suppresses nor guarantees launchd's request.

## Activation lifecycle

Both `clockwork run KEY` and the private launchd entry point resolve only a
stable key. Callers cannot supply executable paths, arguments, environment,
working directory, schedule, timeout, or overlap policy at runtime.

A launchd process waiting at the transition gate has not yet been admitted and
does not pin a definition. After the gate opens it resolves the then-current
binding. Definition identity is pinned only when Clockwork commits the running
activation row.

The broker enters a shared per-key transition gate, refuses a pending binding
transition, pins the selected immutable definition, acquires the exclusive
per-key activation lock, and re-verifies the direct artifacts. It then starts
the same installed Clockwork binary as a blocked execution gate and new
process group leader, records that PID, and only then releases a one-byte
parent pipe.

EOF before that release makes the gate exit without product execution. After
release the gate proves that its PID owns the running activation, re-verifies
the definition, claims and unlinks a private status marker, opens the product
outputs, and `exec`s the registered program or `/bin/sh` profile in place.

A pre-exec or loader failure is written only to that marker and becomes
`start_failed`; Clockwork diagnostics do not enter product output. The
recorded PID is therefore the product PID after exec, and there is no
unrecorded spawned-product window.

The broker waits for that process, forwards termination to its process group,
applies the optional timeout, and records one terminal result. One activation
is one product execution; there is no attempt/retry submodel. The hidden gate
accepts only key plus activation identity and cannot execute unless its own PID
matches the already committed running row.

Terminal states are `start_failed`, `exited`, `signaled`, `timed_out`,
`skipped_overlap`, and `lost`. `running` is nonterminal. Every overlap is
recorded as `skipped_overlap` without starting a child. A scheduled overlap
returns success to launchd; a manual busy run returns `activation_busy` with
exit 1. An unfinished row is
changed to `lost` only after its recorded broker and any recorded child are
both demonstrably absent.

Clockwork records no stdout or stderr body. Definitions name distinct absolute
output paths owned by the product. Each existing canonical parent must have no
symlinks, permit owner write and search access, and prohibit group and other
users from writing. An existing destination must be a private regular file that
the owner can write, with no symbolic or hard links. Clockwork opens both
destinations, verifies different device/inode identities, and appends to them or
creates them with mode 0600. The product owns their content and retention.
Output may not overlap the product release or Clockwork-owned state, log, or
LaunchAgent trees.

## launchd and session limits

Generated plists contain the exact content-addressed installed Clockwork binary
path after checking it against its release manifest, the private stable-key
invocation, the interval or local-calendar schedule,
`HOME`, and Clockwork-owned broker log paths. They do not contain product
executables, product arguments, secret environment, or a product selector.

Clockwork supports interval timers and one daily local-calendar hour/minute.
These are launchd triggers, not a service-level clock. Delivery depends on the
user being logged in, macOS launchd behavior, sleep/wake coalescing, clock and
time-zone changes, resource pressure, TCC and filesystem access, and the
continued existence of the exact release paths. Clockwork specifies no
maximum start delay, catch-up count, fairness, availability percentage, or
delivery SLA.

## Deliberate exclusions

Clockwork has no agent execution, Nucleus submission, daemon, HTTP or network
surface, workflow dependencies, fan-out, product retry engine, backoff, product-work queue,
distributed lock, secret store, output capture, product log rotation, calendar
expressions beyond the documented forms, or system/root service mode.

## Rust interface

`clockwork::api` owns the activation manifest, definition, binding and history
types, their existing codecs, and a typed client for an explicitly selected
Clockwork executable. The CLI serializes these same types. Manifest decoding
is structural; registration still performs the authoritative local-artifact
and scheduling checks. Private plist bookkeeping is excluded from binding
exports.

## Failure boundary

Each schema-two product definition declares the response to an abend. Omission
halts its stable binding until explicit approval; an intentional
`continue-next-activation` exception permits the next activation. Clockwork
observes process failures. Products report terminal domain failures through
`clockwork::api::report_abend` and stop successor admission within their batch.
Expected empty, waiting, deferred, and overlap outcomes remain normal.

One incident closes admission independently of selection and enabled state.
Runtime completion and its abend commit atomically; an in-process report
commits its halt before returning. Binding transitions and compensation do not
snapshot or restore incidents. Resume targets an exact incident and never
retries product work or enables a disabled binding.

A pending Email notification is part of the halt transaction. Broker visits
attempt due notifications before the product gate and after outcomes, including
for other halted or disabled keys. No resident worker is added. Transport never
controls whether the gate remains closed. The fixed payload, five-minute retry
spacing, 120-second process bound, and 23-hour deduplication horizon constrain
notification recovery; later retries need explicit duplicate-risk approval.
Use the CLI contract for commands, fields, privacy, and upgrade requirements.

## EMT boundary

EMT consumes the insertion-ordered incident feed and owns agent diagnosis,
correspondence and delegated email transport. Clockwork retains only optional
notification routes, grace deadlines and delivery ownership. It runs no agent
and stores no diagnosis. Claim and basic-send admission share the notification
lock. EMT must persist its email before claiming it. Unclaimed basic alerts
remain available after 120 seconds; claims do not expire. EMT's own worker
failure uses only the basic path.

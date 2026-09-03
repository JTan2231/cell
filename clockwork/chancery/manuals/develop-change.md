# Change Clockwork

Read Clockwork's product instructions and the exact architecture, CLI,
data-model, installation, semantic-seed, packaging, and Chancery documents for
the behavior being changed. Keep version 0.1 small: immutable strict
definitions, stable bindings, generated current-user LaunchAgents, per-key
admission, one directly supervised child, and runtime history.

The executable boundary is the central invariant. Runtime callers and the
private launchd entry supply one stable `owner/name` key and no process
context. Registration alone accepts the exact release, top-level program or
interpreter/script hashes, literal arguments, exact scrubbed environment,
absolute cwd and output paths, schedule, timeout, and skip-overlap policy.
The recognized-Mach-O product program or executable script stays beneath an
immutable non-symbolic release. Schema one permits only exact, separately
hashed, root-owned `/bin/sh` as an interpreter. Every launch image is canonical, symlink-free, executable,
non-writable by group or world, and reverified before spawn. Never add PATH lookup, mutable selectors, shell strings,
interpolation, implicit shebang choice, inherited environment, or runtime argv.

Keep the attestation claim narrow. Clockwork verifies the registered top-level
launch images at the documented times. It does not attest transitive libraries,
configuration opened later, subprocesses, network peers, same-user tampering
after verification, or product meaning.

Preserve authority:

- Clockwork owns immutable definition and activation identity, binding
  selection, its generated plists, overlap admission, direct supervision, and
  runtime history.
- The product owns release publication, durable work, locks, retry,
  idempotency, secrets, output content and rotation, recovery, and domain
  success.
- launchd owns timer delivery in the current user's GUI session. Clockwork
  publishes the reliance and no delivery SLA.

Each activation remains at most one direct child, not a container of attempts.
Exercise start failure, exit, signal, timeout, scheduled and manual overlap,
termination forwarding, and lost-process proof independently. Never turn exit
zero into `succeeded`, store output bodies, or retry automatically.

Binding transitions use an exclusive stable-key gate while activations use its
shared side plus an exclusive admission lock. They treat database selection,
atomic plist bytes and digest, and launchd loaded state as one operational consistency
unit. Tests should cover first enable, idempotence, update, active refusal,
bootout/bootstrap failures, compensation, and fail-disabled behavior. They
must prove that no recovery leaves old and new schedules intentionally loaded
together.

Use isolated synthetic release roots, state roots, plists, launchd doubles, and
child programs. Fixtures contain no credential, real product path, production
definition, or private output. Errors should name a field or stable ID without
dumping a whole stored environment.

A storage change needs a successor schema, quiescent database-plus-sidecar
backup, explicit migration command, old-state fixture, and database-aware
rollback. Do not migrate during program deployment. A changed immutable
definition meaning receives a new identity rather than an in-place rewrite.

Packaging checks must cover binary/provider version matching, exact
content-addressed release trees, candidate-reader validation of the exact
staged provider before selector mutation, idempotent redeploy, update, foreign
and tampered selector refusal, pre-commit rollback, and selector-only
uninstall without opening runtime state or touching product plists.

Finish with `clockwork/ci.sh` green. `release.sh` creates a commit, tag, and
remote push; the deployer changes installed program selectors; schedule
commands change launchd and product activation state; Semantics registration
and seeding write maintained terminology. Each is a separate effect and none
follows from the development operation without separate authority.

Until `clockwork` is explicitly registered and seeded, query Cell for shared
maintained terminology. After that transition, query the Clockwork repository.
In both cases, code, tests, and product documentation remain authoritative for
behavior.

Stop for a new contract review before adding a daemon, network surface, agent
execution, arbitrary command interface, workflow graph, queue, retry/backoff,
secret storage, product output retention, distributed coordination, system
service, or domain-success policy.

# Clockwork agent instructions

Semantics-Project: clockwork

- Keep Clockwork a short-lived current-user scheduled-activation broker. It is
  not a daemon, arbitrary command runner, workflow engine, retry engine, secret
  store, or product-domain scheduler.
- Until the `clockwork` Semantics project is explicitly registered and seeded,
  use the Cell semantic repository for shared terminology. After registration,
  the Clockwork project's Semantics repository becomes authoritative for
  maintained Clockwork terminology. Code, tests, and product documentation
  remain authoritative for behavior. Never edit Semantics state directly.
- Admit only immutable registered definitions selected by stable bindings.
  Runtime and private launchd entry points accept a stable key, never caller
  supplied executable, argv, environment, working directory, or policy.
- Treat an executable as the registered top-level launch image. Verify its
  pinned direct artifacts, but do not claim to attest transitive libraries,
  configuration, subprocesses, same-user tamper resistance, or product meaning.
- Clockwork owns activation admission, direct-process supervision, generated
  Clockwork LaunchAgents, and runtime history. Each product owns its durable
  work, locks, retries, idempotency, secrets, logs, and domain success.
- Store no product output body or secret. Keep definitions, paths, identifiers,
  and activation metadata private to the current user.
- Update the CLI, architecture, data model, installation guide, packaging, and
  Chancery contracts together when shared behavior changes.
- `release.sh` commits, tags, and pushes, and the macOS deployer changes
  installed selectors and launchd state. Do not invoke either without separate
  authority.
- Every code change must leave `./ci.sh` green.

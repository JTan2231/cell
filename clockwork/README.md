# Clockwork

Clockwork is the current-user broker for scheduled, non-agent activations in
Cell. A product registers one immutable definition, selects it through a stable
`owner/name` binding, and lets a generated macOS LaunchAgent invoke Clockwork
with only that stable key. Clockwork verifies the pinned launch image, admits
at most one activation for the key, supervises the direct child, and records
runtime history.

Clockwork is deliberately smaller than a general job service. It has no
daemon, HTTP API, shell language, workflow graph, retry policy, secret store,
or product-domain success rule. Products keep their durable work, locks,
idempotency, retries, logs, secrets, and interpretation of success.

## Build and check

```sh
./clockwork/ci.sh
cargo build --release --locked --package clockwork
```

## Public shape

```text
clockwork definition register FILE
clockwork definition list
clockwork definition show DEFINITION_DIGEST
clockwork binding switch KEY DEFINITION_DIGEST
clockwork binding disable KEY [--select DEFINITION_DIGEST]
clockwork binding list
clockwork binding show KEY
clockwork run KEY
clockwork history [KEY] [--limit N]
clockwork doctor
```

The installed database defaults to
`~/Library/Application Support/Clockwork/clockwork.db`. The generated
LaunchAgents live in `~/Library/LaunchAgents` and are owned through labels of
the form `org.clockwork.owner.name`.

Start with [the documentation index](docs/README.md). The
[Clockwork Chancery provider](chancery/provider.json) is the release-matched
inventory of supported outward promises. The project-local
[semantic seed](docs/semantics-seed.md) records the implemented vocabulary for
later explicit Semantics registration; it is source only and has not been
registered or seeded by this change.

`release.sh` commits, tags, and pushes. The macOS packaging scripts change
installed selectors. Neither is a build command, and neither should be run
without separate authority.

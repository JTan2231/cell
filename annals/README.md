# Annals

Annals retains source documents and organizes them in named libraries. Each
library has its own instructions, concept graph, exact quotations, and revision
history.

## Example

With a configured library and an authenticated Nucleus service:

```sh
annals integrate report.md
annals stats
```

Integration records an interpretation. Apply a pending reconciliation to change
the corpus. See [integration and application](docs/cli.md#model-assisted-integration).

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Commands and records](docs/cli.md), including [inbox operations](docs/inbox.md)
- [Installation and recovery](docs/system-installation.md)
- [Architecture and graph meaning](docs/architecture.md)
- [Stored records](docs/data-model.md)
- [Search](docs/search.md) and [usage reporting](docs/telemetry.md)
- [Rust interface](docs/rust-api.md)
- [Operating contracts](chancery/annals/provider.json)

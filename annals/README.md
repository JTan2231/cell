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
the corpus. See [integration and application](chancery/annals/manuals/work-integrate.md).

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Product overview and feature map](chancery/annals/overview.md)
- [Library procedures](chancery/annals/manuals/library-operate.md) and [inbox procedures](chancery/annals/manuals/inbox-operate.md)
- [Installation and recovery](chancery/annals/manuals/install-operate.md)
- [Annals Usage](chancery/annals-usage/overview.md)
- [Command navigation](docs/cli.md) and [Rust interface navigation](docs/rust-api.md)

Installed documentation: `chancery product annals`, `chancery show ID`, or
`chancery resolve ID` for a procedure and its required contracts.

# Krisis

Krisis identifies decisions in completed root user turns and delivers decision
accounts to a dedicated Annals library. Annals retains the accepted accounts.
Krisis keeps the coverage and receipts needed to trace and recover delivery.

The command is `krisis`. The source directory and existing state paths retain
the `decisions` name.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Commands and records](docs/cli.md)
- [Installation and recovery](docs/system-installation.md)
- [Architecture](docs/architecture.md) and [stored records](docs/data-model.md)
- [Operating contracts](chancery/provider.json)

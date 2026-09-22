# Semantics

Semantics maintains terminology and its history for registered project folders.
It reads accepted decision accounts from Annals and uses Nucleus to propose
changes. Semantics validates each change before it appends a repository revision.

## Example

```sh
semantics project list
semantics repository show PROJECT
semantics repository search PROJECT TERM
```

Repository reads describe maintained meaning. Product code and documentation
define current runtime behavior.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Commands and repository output](docs/cli.md)
- [Registration, installation, and recovery](docs/system-installation.md)
- [Architecture](docs/architecture.md) and [stored records](docs/data-model.md)
- [Operating contracts](chancery/provider.json)

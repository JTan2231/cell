# Clockwork

Clockwork starts scheduled programs for Cell products. A product registers a
fixed program definition and selects it with an `owner/name` binding.
Clockwork supervises each process and records its outcome. The product owns
its work, retries, and interpretation of success.

## Example

Inspect the configured bindings and recent activations:

```sh
clockwork binding list
clockwork history --limit 20
clockwork doctor
```

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Commands and definition format](docs/cli.md)
- [Installation and recovery](docs/system-installation.md)
- [Architecture and Rust interface](docs/architecture.md)
- [Stored records](docs/data-model.md)
- [Operating contracts](chancery/provider.json)
